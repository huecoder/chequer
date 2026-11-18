package dev.huecoder.chequer;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.SharedPreferences;
import android.content.res.AssetFileDescriptor;
import android.database.Cursor;
import android.net.LocalServerSocket;
import android.net.LocalSocket;
import android.net.LocalSocketAddress;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.os.SystemClock;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.view.DragEvent;
import android.view.InputDevice;
import android.text.InputType;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.provider.OpenableColumns;

import java.io.BufferedReader;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Queue;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.LinkedBlockingQueue;

/**
 * NativeActivity with a bridge to the system file dialog (Storage Access Framework). Rust talks to it over a unix socket
 * in the app folder and waits for the answer, so Rust needs no JNI.
 * Protocol, TAB-separated lines: OPEN mime multiple | SAVE mime name | SYNC path | STYLUS | VIBRATE millis | KEYBOARD 0/1 | IME_POLL | VIEW_POLL | SHARE path | BACK_EXIT | VOLUME_KEYS 0/1. Reply: "uri TAB path" lines and END (UNAVAILABLE - no system dialog opened, ERR - failure), for STYLUS - stylus device ids and END.
 */
public class MainActivity extends NativeActivity {
    private static final int REQUEST_PICK = 4242;
    private static final String SOCKET_FILE = "bridge.sock";
    private static final String MAP_PREFS = "saf-map";
    private static final int COPY_BUFFER = 64 * 1024;
    private static final String BINARY_SUFFIX = ".bin";
    private static final String[] WRITE_MODES = {"rwt", "wt", "w"};
    private static final String ANY_MIME = "*/*";
    private static final String BINARY_MIME = "application/octet-stream";
    private static final String UNAVAILABLE_REPLY = "UNAVAILABLE";
    private static final String SAF_CACHE_DIR = "saf";
    private static final long STALE_CACHE_MILLIS = 14L * 24 * 60 * 60 * 1000;
    private static final long ACCEPT_RETRY_MILLIS = 100;

    private static final String NATIVE_LIBRARY = "chequer_app";
    private static final int TICK_MAX_MILLIS = 15;
    private static final int CLICK_MAX_MILLIS = 40;
    private static final int PEN_DOWN = 0;
    private static final int PEN_MOVE = 1;
    private static final int PEN_UP = 2;
    private static final boolean PEN_FEED = loadNativeLibrary();

    private final BlockingQueue<Intent> results = new LinkedBlockingQueue<>();
    private final Intent cancelled = new Intent();
    private final Intent launchFailed = new Intent();
    private final Queue<String> imeEvents = new ConcurrentLinkedQueue<>();
    private final Queue<String> viewRequests = new ConcurrentLinkedQueue<>();
    private LocalSocket bridgeSocket;
    private volatile boolean captureVolumeKeys;
    private ImeView imeView;

    private static boolean loadNativeLibrary() {
        try {
            System.loadLibrary(NATIVE_LIBRARY);
            return true;
        } catch (UnsatisfiedLinkError missing) {
            return false;
        }
    }

    private static native void nativePenSample(int phase, float x, float y, float pressure, float tilt, float orientation, float ageMs, int barrel);

    private static native void nativePenHover(int active, float x, float y, int barrel);

    private static native void nativeVolumeKey(int up);

    private static int barrelBit(MotionEvent event) {
        return (event.getButtonState() & (MotionEvent.BUTTON_STYLUS_PRIMARY | MotionEvent.BUTTON_STYLUS_SECONDARY)) != 0 ? 1 : 0;
    }

    private static void forwardPenHover(MotionEvent event) {
        if (!isPen(event, 0)) {
            return;
        }
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_HOVER_ENTER:
            case MotionEvent.ACTION_HOVER_MOVE:
                nativePenHover(1, event.getX(), event.getY(), barrelBit(event));
                break;
            case MotionEvent.ACTION_HOVER_EXIT:
                nativePenHover(0, 0, 0, 0);
                break;
            default:
                break;
        }
    }

    @Override
    public boolean dispatchGenericMotionEvent(MotionEvent event) {
        if (PEN_FEED) {
            forwardPenHover(event);
        }
        return super.dispatchGenericMotionEvent(event);
    }

    private boolean isCapturedVolumeKey(int keyCode) {
        return captureVolumeKeys && (keyCode == KeyEvent.KEYCODE_VOLUME_UP || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN);
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (!isCapturedVolumeKey(keyCode)) {
            return super.onKeyDown(keyCode, event);
        }
        if (PEN_FEED && event.getRepeatCount() == 0) {
            nativeVolumeKey(keyCode == KeyEvent.KEYCODE_VOLUME_UP ? 1 : 0);
        }
        return true;
    }

    @Override
    public boolean onKeyUp(int keyCode, KeyEvent event) {
        return isCapturedVolumeKey(keyCode) || super.onKeyUp(keyCode, event);
    }

    private static float ageMillis(long eventTime) {
        return SystemClock.uptimeMillis() - eventTime;
    }

    @Override
    public boolean dispatchTouchEvent(MotionEvent event) {
        if (PEN_FEED) {
            forwardPenSamples(event);
        }
        return super.dispatchTouchEvent(event);
    }

    private static boolean isPen(MotionEvent event, int pointer) {
        int tool = event.getToolType(pointer);
        return tool == MotionEvent.TOOL_TYPE_STYLUS || tool == MotionEvent.TOOL_TYPE_ERASER;
    }

    private static int penPhase(MotionEvent event, int pointer) {
        boolean changing = pointer == event.getActionIndex();
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                return PEN_DOWN;
            case MotionEvent.ACTION_POINTER_DOWN:
                return changing ? PEN_DOWN : PEN_MOVE;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_CANCEL:
                return PEN_UP;
            case MotionEvent.ACTION_POINTER_UP:
                return changing ? PEN_UP : PEN_MOVE;
            default:
                return PEN_MOVE;
        }
    }

    private static void forwardPenSamples(MotionEvent event) {
        for (int pointer = 0; pointer < event.getPointerCount(); pointer++) {
            if (!isPen(event, pointer)) {
                continue;
            }
            int phase = penPhase(event, pointer);
            if (phase == PEN_MOVE) {
                for (int past = 0; past < event.getHistorySize(); past++) {
                    nativePenSample(PEN_MOVE, event.getHistoricalX(pointer, past), event.getHistoricalY(pointer, past), event.getHistoricalPressure(pointer, past),
                        event.getHistoricalAxisValue(MotionEvent.AXIS_TILT, pointer, past), event.getHistoricalAxisValue(MotionEvent.AXIS_ORIENTATION, pointer, past), ageMillis(event.getHistoricalEventTime(past)), barrelBit(event));
                }
            }
            nativePenSample(phase, event.getX(pointer), event.getY(pointer), event.getPressure(pointer), event.getAxisValue(MotionEvent.AXIS_TILT, pointer), event.getAxisValue(MotionEvent.AXIS_ORIENTATION, pointer), ageMillis(event.getEventTime()), barrelBit(event));
        }
    }

    @Override
    protected void onCreate(Bundle state) {
        startBridge();
        super.onCreate(state);
        dropStaleCache();
        imeView = new ImeView(this);
        addContentView(imeView, new ViewGroup.LayoutParams(1, 1));
        getWindow().getDecorView().setOnDragListener(new View.OnDragListener() {
            @Override
            public boolean onDrag(View view, DragEvent event) {
                return receiveDrop(event);
            }
        });
        receiveViewIntent(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        receiveViewIntent(intent);
    }

    private static Uri receivedUri(Intent intent) {
        if (intent == null) {
            return null;
        }
        if (Intent.ACTION_VIEW.equals(intent.getAction())) {
            return intent.getData();
        }
        return Intent.ACTION_SEND.equals(intent.getAction()) ? (Uri) intent.getParcelableExtra(Intent.EXTRA_STREAM) : null;
    }

    /** Images dropped on the window from another app (split screen, DeX): copied to the cache and queued for VIEW_POLL. */
    private boolean receiveDrop(DragEvent event) {
        switch (event.getAction()) {
            case DragEvent.ACTION_DRAG_STARTED:
                return event.getClipDescription() != null && event.getClipDescription().hasMimeType("image/*");
            case DragEvent.ACTION_DROP:
                final ClipData clip = event.getClipData();
                if (clip == null) {
                    return false;
                }
                requestDragAndDropPermissions(event);
                new Thread(new Runnable() {
                    @Override
                    public void run() {
                        for (int i = 0; i < clip.getItemCount(); i++) {
                            Uri uri = clip.getItemAt(i).getUri();
                            try {
                                if (uri != null) {
                                    viewRequests.add(copyToCache(uri) + "\t0");
                                }
                            } catch (Exception ignored) {
                            }
                        }
                    }
                }, "chequer-drop").start();
                return true;
            default:
                return true;
        }
    }

    /** A file opened from another app: copied to the cache, its path and write permission are queued for VIEW_POLL. */
    private void receiveViewIntent(Intent intent) {
        final Uri uri = receivedUri(intent);
        if (uri == null) {
            return;
        }
        final boolean writable = Intent.ACTION_VIEW.equals(intent.getAction()) && checkCallingOrSelfUriPermission(uri, Intent.FLAG_GRANT_WRITE_URI_PERMISSION) == PackageManager.PERMISSION_GRANTED;
        new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    String path = copyToCache(uri);
                    if (writable) {
                        remember(path, uri);
                    }
                    viewRequests.add(path + "\t" + (writable ? "1" : "0"));
                } catch (Exception ignored) {
                }
            }
        }, "chequer-view").start();
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        if (request != REQUEST_PICK) {
            super.onActivityResult(request, result, data);
            return;
        }
        results.add(result == RESULT_OK && data != null ? data : cancelled);
    }

    private void startBridge() {
        final File socket = new File(getFilesDir(), SOCKET_FILE);
        Thread server = new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    socket.delete();
                    bridgeSocket = new LocalSocket();
                    bridgeSocket.bind(new LocalSocketAddress(socket.getAbsolutePath(), LocalSocketAddress.Namespace.FILESYSTEM));
                    LocalServerSocket listener = new LocalServerSocket(bridgeSocket.getFileDescriptor());
                    while (true) {
                        try {
                            serveInBackground(listener.accept());
                        } catch (Exception failedAccept) {
                            Thread.sleep(ACCEPT_RETRY_MILLIS);
                        }
                    }
                } catch (Exception ignored) {
                }
            }
        }, "chequer-bridge");
        server.setDaemon(true);
        server.start();
    }

    private void serveInBackground(final LocalSocket client) {
        new Thread(new Runnable() {
            @Override
            public void run() {
                try {
                    serve(client);
                } catch (Exception failure) {
                    try {
                        PrintWriter out = new PrintWriter(client.getOutputStream(), true);
                        out.println("ERR\t" + failure);
                        out.println("END");
                    } catch (Exception ignored) {
                    }
                } finally {
                    try {
                        client.close();
                    } catch (Exception ignored) {
                    }
                }
            }
        }, "chequer-client").start();
    }

    private void serve(LocalSocket client) throws Exception {
        BufferedReader in = new BufferedReader(new InputStreamReader(client.getInputStream(), "UTF-8"));
        PrintWriter out = new PrintWriter(client.getOutputStream(), true);
        String line = in.readLine();
        if (line == null) {
            return;
        }
        String[] parts = line.split("\t", -1);
        switch (parts[0]) {
            case "OPEN":
                List<Uri> opened = pick(openAttempts(parts[1], parts[2].equals("1")));
                if (opened == null) {
                    out.println(UNAVAILABLE_REPLY);
                    break;
                }
                for (Uri uri : opened) {
                    String path = copyToCache(uri);
                    remember(path, uri);
                    out.println(uri + "\t" + path);
                }
                break;
            case "SAVE":
                List<Uri> created = pick(saveAttempts(parts[1], parts[2]));
                if (created == null) {
                    out.println(UNAVAILABLE_REPLY);
                    break;
                }
                for (Uri picked : created) {
                    Uri uri = restoreRequestedName(picked, parts[2]);
                    String path = cachePath(uri);
                    remember(path, uri);
                    out.println(uri + "\t" + path);
                }
                break;
            case "SYNC":
                out.println(syncBack(parts[1]));
                break;
            case "SHARE":
                share(parts[1]);
                break;
            case "BACK_EXIT":
                runOnUiThread(new Runnable() {
                    @Override
                    public void run() {
                        moveTaskToBack(true);
                    }
                });
                break;
            case "VOLUME_KEYS":
                captureVolumeKeys = parts[1].equals("1");
                break;
            case "VIBRATE":
                vibrate(Integer.parseInt(parts[1]));
                break;
            case "KEYBOARD":
                setKeyboard(parts[1].equals("1"));
                break;
            case "IME_POLL":
                String typed;
                while ((typed = imeEvents.poll()) != null) {
                    out.println(typed);
                }
                break;
            case "VIEW_POLL":
                String viewed;
                while ((viewed = viewRequests.poll()) != null) {
                    out.println(viewed);
                }
                break;
            case "STYLUS":
                for (int id : InputDevice.getDeviceIds()) {
                    if (isStylusOnly(InputDevice.getDevice(id))) {
                        out.println(id);
                    }
                }
                break;
            default:
                break;
        }
        out.println("END");
    }

    private static VibrationEffect hapticEffect(int millis) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            return VibrationEffect.createOneShot(millis, VibrationEffect.DEFAULT_AMPLITUDE);
        }
        if (millis <= TICK_MAX_MILLIS) {
            return VibrationEffect.createPredefined(VibrationEffect.EFFECT_TICK);
        }
        return VibrationEffect.createPredefined(millis <= CLICK_MAX_MILLIS ? VibrationEffect.EFFECT_CLICK : VibrationEffect.EFFECT_HEAVY_CLICK);
    }

    private void share(String path) {
        Uri uri = Uri.parse("content://" + ShareProvider.AUTHORITY + "/" + Uri.encode(new File(path).getName()));
        final Intent send = new Intent(Intent.ACTION_SEND);
        send.setType("image/png");
        send.putExtra(Intent.EXTRA_STREAM, uri);
        send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                startActivity(Intent.createChooser(send, null));
            }
        });
    }

    private void vibrate(int millis) {
        Vibrator vibrator = (Vibrator) getSystemService(VIBRATOR_SERVICE);
        if (vibrator != null && vibrator.hasVibrator()) {
            vibrator.vibrate(hapticEffect(millis));
        }
    }

    private void setKeyboard(final boolean visible) {
        runOnUiThread(new Runnable() {
            @Override
            public void run() {
                InputMethodManager input = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
                if (visible) {
                    showKeyboardWhenFocused(input);
                } else {
                    input.hideSoftInputFromWindow(imeView.getWindowToken(), 0);
                }
            }
        });
    }

    private void showKeyboardWhenFocused(final InputMethodManager input) {
        imeView.requestFocus();
        imeView.post(new Runnable() {
            @Override
            public void run() {
                input.restartInput(imeView);
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    WindowInsetsController controller = imeView.getWindowInsetsController();
                    if (controller != null) {
                        controller.show(WindowInsets.Type.ime());
                    }
                }
                input.showSoftInput(imeView, 0);
            }
        });
    }

    private static String hex(CharSequence text) {
        StringBuilder out = new StringBuilder();
        for (byte b : text.toString().getBytes(StandardCharsets.UTF_8)) {
            out.append(String.format("%02x", b));
        }
        return out.toString();
    }

    /**
     * Invisible input field: NativeActivity gives the system keyboard no InputConnection, so letters, non-Latin text and
     * deletion arrive here and Rust picks them up with IME_POLL. Events: TEXT hex(utf-8) | KEY BACKSPACE | KEY ENTER.
     * Suggestions are off so every letter reaches the app at once.
     */
    private final class ImeView extends View {
        ImeView(Context context) {
            super(context);
            setFocusable(true);
            setFocusableInTouchMode(true);
        }

        @Override
        public boolean onCheckIsTextEditor() {
            return true;
        }

        @Override
        public boolean onKeyPreIme(int keyCode, KeyEvent event) {
            if (keyCode != KeyEvent.KEYCODE_BACK) {
                return super.onKeyPreIme(keyCode, event);
            }
            if (event.getAction() == KeyEvent.ACTION_UP) {
                InputMethodManager input = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
                input.hideSoftInputFromWindow(getWindowToken(), 0);
                imeEvents.add("KEY\tHIDE");
            }
            return true;
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo info) {
            info.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS | InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD;
            info.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN | EditorInfo.IME_FLAG_NO_EXTRACT_UI | EditorInfo.IME_ACTION_NONE;
            return new BaseInputConnection(this, false) {
                private int composing = 0;

                private void backspaces(int count) {
                    for (int i = 0; i < count; i++) {
                        imeEvents.add("KEY\tBACKSPACE");
                    }
                }

                private void type(CharSequence text) {
                    backspaces(composing);
                    composing = 0;
                    if (text.length() > 0) {
                        imeEvents.add("TEXT\t" + hex(text));
                    }
                }

                @Override
                public boolean commitText(CharSequence text, int newCursorPosition) {
                    type(text);
                    return true;
                }

                @Override
                public boolean setComposingText(CharSequence text, int newCursorPosition) {
                    type(text);
                    composing = text.length();
                    return true;
                }

                @Override
                public boolean finishComposingText() {
                    composing = 0;
                    return true;
                }

                @Override
                public boolean deleteSurroundingText(int beforeLength, int afterLength) {
                    backspaces(Math.max(beforeLength, 1));
                    return true;
                }

                @Override
                public boolean sendKeyEvent(KeyEvent event) {
                    if (event.getAction() == KeyEvent.ACTION_DOWN) {
                        if (event.getKeyCode() == KeyEvent.KEYCODE_DEL) {
                            imeEvents.add("KEY\tBACKSPACE");
                        } else if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
                            imeEvents.add("KEY\tENTER");
                        } else if (event.getUnicodeChar() != 0) {
                            imeEvents.add("TEXT\t" + hex(new String(Character.toChars(event.getUnicodeChar()))));
                        }
                    }
                    return true;
                }

                @Override
                public boolean performEditorAction(int editorAction) {
                    imeEvents.add("KEY\tENTER");
                    return true;
                }
            };
        }
    }

    /** A separate pen digitizer (S Pen): a STYLUS source without TOUCHSCREEN, so its events never mix with fingers. */
    private static boolean isStylusOnly(InputDevice device) {
        if (device == null) {
            return false;
        }
        int sources = device.getSources();
        return (sources & InputDevice.SOURCE_STYLUS) == InputDevice.SOURCE_STYLUS && (sources & InputDevice.SOURCE_TOUCHSCREEN) != InputDevice.SOURCE_TOUCHSCREEN;
    }

    private Intent documentIntent(String action, String mime, String name, boolean multiple) {
        Intent intent = new Intent(action);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType(mime);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        if (name != null) {
            intent.putExtra(Intent.EXTRA_TITLE, name);
        }
        if (multiple) {
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
        }
        return intent;
    }

    private List<Intent> openAttempts(String mime, boolean multiple) {
        List<Intent> attempts = new ArrayList<>();
        attempts.add(documentIntent(Intent.ACTION_OPEN_DOCUMENT, mime, null, multiple));
        attempts.add(Intent.createChooser(documentIntent(Intent.ACTION_GET_CONTENT, mime, null, multiple), null));
        if (!ANY_MIME.equals(mime)) {
            attempts.add(documentIntent(Intent.ACTION_OPEN_DOCUMENT, ANY_MIME, null, multiple));
            Intent gallery = new Intent(Intent.ACTION_PICK, MediaStore.Images.Media.EXTERNAL_CONTENT_URI);
            gallery.setType(mime);
            if (multiple) {
                gallery.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
            }
            attempts.add(gallery);
        }
        attempts.add(Intent.createChooser(documentIntent(Intent.ACTION_GET_CONTENT, ANY_MIME, null, multiple), null));
        return attempts;
    }

    private List<Intent> saveAttempts(String mime, String name) {
        List<Intent> attempts = new ArrayList<>();
        attempts.add(documentIntent(Intent.ACTION_CREATE_DOCUMENT, mime, name, false));
        if (!BINARY_MIME.equals(mime)) {
            attempts.add(documentIntent(Intent.ACTION_CREATE_DOCUMENT, BINARY_MIME, name, false));
        }
        return attempts;
    }

    private synchronized List<Uri> pick(List<Intent> attempts) throws InterruptedException {
        Intent data = launchFailed;
        for (final Intent intent : attempts) {
            results.clear();
            runOnUiThread(new Runnable() {
                @Override
                public void run() {
                    try {
                        startActivityForResult(intent, REQUEST_PICK);
                    } catch (Exception e) {
                        results.add(launchFailed);
                    }
                }
            });
            data = results.take();
            if (data != launchFailed) {
                break;
            }
        }
        if (data == launchFailed) {
            return null;
        }
        List<Uri> uris = new ArrayList<>();
        ClipData clip = data.getClipData();
        if (clip != null) {
            for (int i = 0; i < clip.getItemCount(); i++) {
                uris.add(clip.getItemAt(i).getUri());
            }
        } else if (data.getData() != null) {
            uris.add(data.getData());
        }
        for (Uri uri : uris) {
            try {
                getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            } catch (Exception ignored) {
            }
        }
        return uris;
    }

    private String displayName(Uri uri) {
        Cursor cursor = getContentResolver().query(uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null);
        try {
            if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) {
                return cursor.getString(0).replace('/', '_');
            }
        } finally {
            if (cursor != null) {
                cursor.close();
            }
        }
        String segment = uri.getLastPathSegment();
        return segment != null ? segment.substring(segment.lastIndexOf('/') + 1) : "file";
    }

    /** The same SAF location always maps to the same cache path, so saving again writes to the same file. */
    private String cachePath(Uri uri) {
        File dir = new File(getCacheDir(), SAF_CACHE_DIR + "/" + Integer.toHexString(uri.toString().hashCode()));
        dir.mkdirs();
        return new File(dir, displayName(uri)).getAbsolutePath();
    }

    private String copyToCache(Uri uri) throws Exception {
        String path = cachePath(uri);
        try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(path)) {
            copy(in, out);
        }
        return path;
    }

    /** SAF copies pile up in the cache: files older than two weeks are deleted along with their mapping. */
    private void dropStaleCache() {
        new Thread(new Runnable() {
            @Override
            public void run() {
                SharedPreferences.Editor forget = getSharedPreferences(MAP_PREFS, Context.MODE_PRIVATE).edit();
                File[] dirs = new File(getCacheDir(), SAF_CACHE_DIR).listFiles();
                for (File dir : dirs != null ? dirs : new File[0]) {
                    File[] files = dir.listFiles();
                    for (File file : files != null ? files : new File[0]) {
                        if (System.currentTimeMillis() - file.lastModified() > STALE_CACHE_MILLIS && file.delete()) {
                            forget.remove(file.getAbsolutePath());
                        }
                    }
                    dir.delete();
                }
                forget.apply();
                File[] shared = new File(getFilesDir(), ShareProvider.DIR).listFiles();
                for (File file : shared != null ? shared : new File[0]) {
                    if (System.currentTimeMillis() - file.lastModified() > STALE_CACHE_MILLIS) {
                        file.delete();
                    }
                }
            }
        }, "chequer-cache").start();
    }

    private String syncBack(String path) {
        String uri = getSharedPreferences(MAP_PREFS, Context.MODE_PRIVATE).getString(path, null);
        if (uri == null) {
            return "NONE";
        }
        File source = new File(path);
        String failure = "";
        for (String mode : WRITE_MODES) {
            try {
                writeThrough(source, Uri.parse(uri), mode);
                long stored = storedLength(Uri.parse(uri));
                if (stored == AssetFileDescriptor.UNKNOWN_LENGTH || stored == source.length()) {
                    return "OK";
                }
                failure = "ERR\t" + mode + ": wrote " + stored + " of " + source.length() + " bytes";
            } catch (Exception e) {
                failure = "ERR\t" + mode + ": " + e;
            }
        }
        return failure;
    }

    private void writeThrough(File source, Uri target, String mode) throws Exception {
        ParcelFileDescriptor descriptor = getContentResolver().openFileDescriptor(target, mode);
        if (descriptor == null) {
            throw new java.io.IOException("the provider did not open the file for writing");
        }
        try (InputStream in = new FileInputStream(source); FileOutputStream out = new FileOutputStream(descriptor.getFileDescriptor())) {
            copy(in, out);
            out.flush();
            try {
                out.getFD().sync();
            } catch (java.io.IOException notSeekable) {
            }
        } finally {
            descriptor.close();
        }
    }

    private long storedLength(Uri target) {
        try (AssetFileDescriptor stored = getContentResolver().openAssetFileDescriptor(target, "r")) {
            return stored != null ? stored.getLength() : AssetFileDescriptor.UNKNOWN_LENGTH;
        } catch (Exception e) {
            return AssetFileDescriptor.UNKNOWN_LENGTH;
        }
    }

    /** Some providers save octet-stream files as ".bin" or cut the extension: restore the requested name. */
    private Uri restoreRequestedName(Uri uri, String requested) {
        String shown = displayName(uri);
        String wanted = null;
        if (shown.equals(requested + BINARY_SUFFIX)) {
            wanted = requested;
        } else if (shown.indexOf('.') < 0 && requested.lastIndexOf('.') >= 0) {
            wanted = shown + requested.substring(requested.lastIndexOf('.'));
        }
        if (wanted == null) {
            return uri;
        }
        try {
            Uri renamed = DocumentsContract.renameDocument(getContentResolver(), uri, wanted);
            return renamed != null ? renamed : uri;
        } catch (Exception e) {
            return uri;
        }
    }

    private void remember(String path, Uri uri) {
        getSharedPreferences(MAP_PREFS, Context.MODE_PRIVATE).edit().putString(path, uri.toString()).apply();
    }

    private static void copy(InputStream in, OutputStream out) throws Exception {
        byte[] buffer = new byte[COPY_BUFFER];
        int read;
        while ((read = in.read(buffer)) > 0) {
            out.write(buffer, 0, read);
        }
    }
}

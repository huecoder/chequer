use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{OnceLock, RwLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    Ru,
    En,
}

static CURRENT_LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    CURRENT_LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if CURRENT_LANG.load(Ordering::Relaxed) == Lang::En as u8 { Lang::En } else { Lang::Ru }
}

pub fn lang_code() -> &'static str {
    match lang() {
        Lang::Ru => "ru",
        Lang::En => "en",
    }
}

pub fn t(text: Text) -> &'static str {
    text.get(lang())
}

#[derive(Clone, Copy, Debug)]
pub struct TextEntry {
    key: &'static str,
    ru: &'static str,
    en: &'static str,
}

impl TextEntry {
    pub const fn new(key: &'static str, ru: &'static str, en: &'static str) -> Self {
        Self { key, ru, en }
    }

    fn get(self, lang: Lang) -> &'static str {
        match lang {
            Lang::Ru => self.ru,
            Lang::En => self.en,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextScope {
    Effect,
    Param,
    Option,
    Command,
    Tool,
    ToolHint,
}

impl TextScope {
    fn key(self, name: &str) -> String {
        let prefix = match self {
            TextScope::Effect => "effect",
            TextScope::Param => "param",
            TextScope::Option => "option",
            TextScope::Command => "command",
            TextScope::Tool => "tool",
            TextScope::ToolHint => "tool_hint",
        };
        format!("{prefix}.{name}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplicateTextKey(pub &'static str);

fn registered_texts() -> &'static RwLock<HashMap<&'static str, TextEntry>> {
    static TEXTS: OnceLock<RwLock<HashMap<&'static str, TextEntry>>> = OnceLock::new();
    TEXTS.get_or_init(Default::default)
}

pub fn register_texts(entries: &'static [TextEntry]) -> Result<(), DuplicateTextKey> {
    let mut texts = registered_texts().write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut incoming: Vec<&'static str> = Vec::with_capacity(entries.len());
    for entry in entries {
        if texts.contains_key(entry.key) || incoming.contains(&entry.key) {
            return Err(DuplicateTextKey(entry.key));
        }
        incoming.push(entry.key);
    }
    texts.extend(entries.iter().map(|entry| (entry.key, *entry)));
    Ok(())
}

pub fn text_or_key(scope: TextScope, name: &'static str) -> &'static str {
    let texts = registered_texts().read().unwrap_or_else(|poisoned| poisoned.into_inner());
    texts.get(scope.key(name).as_str()).map_or(name, |entry| entry.get(lang()))
}

macro_rules! texts {
    ($($name:ident => $ru:literal, $en:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Text { $($name),* }

        impl Text {
            pub fn get(self, lang: Lang) -> &'static str {
                match (self, lang) {
                    $( (Text::$name, Lang::Ru) => $ru, (Text::$name, Lang::En) => $en, )*
                }
            }
        }
    };
}

texts! {
    MenuFile => "Файл", "File";
    MenuEdit => "Правка", "Edit";
    MenuView => "Вид", "View";
    MenuImage => "Изображение", "Image";
    MenuLayers => "Слои", "Layers";
    Timeline => "Кадры", "Timeline";
    NewFrame => "Дублировать выделенные кадры  Ctrl+D", "Duplicate selected frames  Ctrl+D";
    NewEmptyFrame => "Новый пустой кадр", "New empty frame";
    DeleteFrame => "Удалить выделенные кадры  Shift+Del", "Delete selected frames  Shift+Del";
    MoveFrameLeft => "Сдвинуть кадры влево  Alt+←", "Move frames left  Alt+←";
    MoveFrameRight => "Сдвинуть кадры вправо  Alt+→", "Move frames right  Alt+→";
    ReverseFrames => "Развернуть порядок кадров (если выделен один - всех)", "Reverse frame order (all if one is selected)";
    ClearCels => "Очистить ячейки  Del", "Clear cels  Del";
    CopyCels => "Копировать ячейки  Ctrl+C", "Copy cels  Ctrl+C";
    PasteCels => "Вставить ячейки  Ctrl+V", "Paste cels  Ctrl+V";
    SelectAllFrames => "Выделить все кадры  Ctrl+A", "Select all frames  Ctrl+A";
    FrameDurationLabel => "Длительность кадра", "Frame duration";
    FirstFrame => "К первому кадру", "First frame";
    PrevFrame => "Предыдущий кадр ,", "Previous frame ,";
    NextFrame => "Следующий кадр .", "Next frame .";
    LastFrame => "К последнему кадру", "Last frame";
    PlayAnimation => "Играть", "Play";
    PauseAnimation => "Пауза", "Pause";
    LoopAnimation => "Зациклить", "Loop";
    OnionSkin => "Просвечивание соседних кадров", "Onion skin";
    CellsCompact => "Компактные ячейки (точки)", "Compact cells (dots)";
    CellsFull => "Крупные ячейки (миниатюры)", "Full cells (thumbnails)";
    AnimationSettings => "Настройки анимации", "Animation settings";
    AnimationPlayback => "Воспроизведение", "Playback";
    AnimationFps => "Кадров в секунду", "Frames per second";
    ApplyToAllFrames => "Ко всем кадрам", "To all frames";
    AnimationMixedFps => "Сейчас у кадров разная длительность.", "Frames currently have different durations.";
    OnionSection => "Просвечивание", "Onion skin";
    OnionEnabled => "Включить", "Enable";
    OnionBefore => "Кадров до", "Frames before";
    OnionAfter => "Кадров после", "Frames after";
    OnionOpacity => "Прозрачность, %", "Opacity, %";
    OnionColorBefore => "Цвет прошлых", "Past color";
    OnionColorAfter => "Цвет будущих", "Future color";
    AnimationKeysSection => "Клавиши и колесо", "Keys and wheel";
    SpacePlays => "Пробел - пуск и пауза", "Space - play and pause";
    ArrowsNavigate => "Стрелки - кадры и слои", "Arrows - frames and layers";
    WheelNavigates => "Колесо - листать кадры", "Wheel - step through frames";
    FramesTitle => "Сохранение без анимации", "Saving without animation";
    SaveFramesWarning => "В этом формате сохранится только текущий кадр.", "This format keeps only the current frame.";
    MenuAdjustments => "Коррекция", "Adjustments";
    MenuEffects => "Эффекты", "Effects";
    MenuPlugins => "Плагины", "Plugins";
    StartupFailed => "Chequer не удалось запустить", "Chequer failed to start";
    New => "Создать...", "New...";
    Open => "Открыть...", "Open...";
    OpenRecent => "Недавние файлы", "Open Recent";
    Save => "Сохранить", "Save";
    SaveAs => "Сохранить как...", "Save As...";
    SaveAll => "Сохранить все", "Save All";
    Close => "Закрыть", "Close";
    Exit => "Выход", "Exit";
    MenuSplash => "Заставка", "Splash Screen";
    WelcomeNewImage => "Новое изображение", "New Image";
    WelcomeOpenImage => "Открыть изображение", "Open Image";
    WelcomeNoRecent => "Пока ничего не открывали", "Nothing opened yet";
    Undo => "Отменить", "Undo";
    Redo => "Вернуть", "Redo";
    Cut => "Вырезать", "Cut";
    Copy => "Копировать", "Copy";
    CopyMerged => "Копировать (все слои)", "Copy Merged";
    Paste => "Вставить", "Paste";
    PasteNewLayer => "Вставить в новый слой", "Paste Into New Layer";
    EraseSelection => "Очистить выделение", "Erase Selection";
    FillSelection => "Заполнить выделение", "Fill Selection";
    InvertSelection => "Обратить выделение", "Invert Selection";
    SelectAll => "Выделить все", "Select All";
    Deselect => "Отменить выделение", "Deselect";
    ZoomIn => "Увеличить", "Zoom In";
    ZoomOut => "Уменьшить", "Zoom Out";
    ZoomToWindow => "По размеру окна", "Zoom to Window";
    ZoomToSelection => "По выделению", "Zoom to Selection";
    ActualSize => "Истинный размер", "Actual Size";
    PixelGrid => "Сетка", "Pixel Grid";
    SeamlessMode => "Бесшовная текстура", "Seamless Tiling";
    OptTileCopies => "Копии:", "Copies:";
    OptTileFrame => "Рамка", "Frame";
    CropToSelection => "Обрезать по выделению", "Crop to Selection";
    ResizeImage => "Изменить размер...", "Resize...";
    CanvasSize => "Размер полотна...", "Canvas Size...";
    OffsetHalf => "Сдвинуть на половину со склейкой", "Offset by Half (Wrap)";
    FlipHorizontal => "Перевернуть горизонтально", "Flip Horizontal";
    FlipVertical => "Перевернуть вертикально", "Flip Vertical";
    RotateCw => "Повернуть по часовой стрелке на 90°", "Rotate 90° Clockwise";
    RotateCcw => "Повернуть против часовой стрелки на 90°", "Rotate 90° Counter-Clockwise";
    Rotate180 => "Повернуть на 180°", "Rotate 180°";
    Flatten => "Объединить все слои", "Flatten";
    AddLayer => "Добавить новый слой", "Add New Layer";
    DeleteLayer => "Удалить слой", "Delete Layer";
    DuplicateLayer => "Дублировать слой", "Duplicate Layer";
    MergeLayerDown => "Слить с нижним слоем", "Merge Layer Down";
    MoveLayerUp => "Поднять слой", "Move Layer Up";
    MoveLayerDown => "Опустить слой", "Move Layer Down";
    LayerProperties => "Свойства слоя...", "Layer Properties...";
    InvertColors => "Инвертировать цвета", "Invert Colors";
    BlackAndWhite => "Сделать черно-белым", "Black and White";
    Tools => "Инструменты", "Tools";
    History => "Журнал", "History";
    Layers => "Слои", "Layers";
    Colors => "Цвет", "Color";
    Swatches => "Палитра", "Palette";
    Settings => "Настройки", "Settings";
    About => "О программе", "About";
    Language => "Язык", "Language";
    UiScale => "Масштаб", "Scale";
    Font => "Шрифт", "Font";
    ResampleAnisotropic => "Анизотропная", "Anisotropic";
    OptRotation => "Поворот", "Rotation";
    OptMoveMethod => "Метод:", "Method:";
    MoveMethodTransform => "Масштаб и поворот", "Scale and rotate";
    MoveMethodPerspective => "Перспектива", "Perspective";
    RotationPlain => "Ближайший сосед", "Nearest neighbor";
OptLassoSmoothing => "Сглаживание", "Smoothing";
LassoClassic => "Классическое", "Classic";
LassoFull => "Полное", "Full";
LassoOff => "Нет", "None";
LineStraight => "Прямая", "Straight";
LineSmooth => "Кривая", "Curve";
    RotationFast => "Fast Rotation", "Fast Rotation";
    RotationRotSprite => "RotSprite", "RotSprite";
    FontBuiltin => "Встроенный", "Built-in";
    FontImport => "Импорт шрифта...", "Import font...";
    FontLoading => "Загрузка списка...", "Loading the list...";
    Grain => "Зерно", "Grain";
    GrainEnabled => "Декоративный шум", "Decorative noise";
    ResetWindowSize => "Сбросить размеры окна", "Reset window size";
    UiScaleAuto => "Как в системе", "System";
SettingsGeneral => "Общие", "General";
SettingsThemes => "Темы", "Themes";
ThemeSettings => "Настройки", "Settings";
ThemesReload => "Обновить список", "Reload list";
GroupTheme => "Тема", "Theme";
GroupThemeOptions => "Параметры темы", "Theme options";
GroupThemeColors => "Цвета", "Colors";
ThemesFolder => "Папка своих тем:", "Custom themes folder:";
SettingsGestures => "Жесты", "Gestures";
GroupTrackpad => "Трекпад", "Trackpad";
TrackpadMode => "Режим", "Mode";
TrackpadAuto => "Авто (напрямую)", "Auto (raw)";
CheckerLight => "Шахматка светлая", "Checker light";
CheckerDark => "Шахматка тёмная", "Checker dark";
CheckerFromTheme => "Как в теме", "From theme";
ZoomOutFilter => "Отдаление", "Zoom out";
ZoomOutPixel => "По пикселям", "Nearest pixel";
ZoomOutSmooth => "Со сглаживанием", "Smoothed";
TrackpadSystem => "Системный (Ctrl+колесо)", "System (Ctrl+wheel)";
PinchSensitivity => "Щипок, %", "Pinch, %";
GestureRotate => "Вращение двумя пальцами", "Two-finger rotate";
GestureSwipes => "Свайпы тремя пальцами", "Three-finger swipes";
HintText => "Печатать текст", "Type text";
HText => "Текст", "Text";
TextFont => "Шрифт:", "Font:";
TextFontDefault => "Системный (без засечек)", "System (sans-serif)";
TextFontBrowse => "Все шрифты с образцами...", "All fonts with samples...";
FontPickerTitle => "Выбор шрифта", "Choose a font";
FontSearchHint => "Поиск по названию", "Search by name";
FontSampleText => "Aa Бб Qq 0123 Съешь ещё булок", "Aa Bb Qq 0123 The quick brown fox";
FontNoMatches => "Ничего не найдено", "Nothing found";
TextSize => "Размер:", "Size:";
TextBold => "Полужирный", "Bold";
TextItalic => "Курсив", "Italic";
TextUnderline => "Подчёркнутый", "Underline";
TextStrikethrough => "Зачёркнутый", "Strikethrough";
TextAlignLeft => "По левому краю", "Align left";
TextAlignCenter => "По центру", "Align center";
TextAlignRight => "По правому краю", "Align right";
TextCenterOnCanvas => "Поставить надпись по центру холста", "Center the text on the canvas";
TextHinting => "Хинтинг", "Hinting";
TextHintingTip => "Подгонка контуров под пиксельную сетку", "Snap outlines to the pixel grid";
TextSmoothing => "Сглаживание:", "Smoothing:";
SmoothNone => "Нет (пиксельный)", "None (pixelated)";
SmoothGrayscale => "Оттенки серого", "Grayscale";
SmoothLcdRgb => "LCD (ClearType, RGB)", "LCD (ClearType, RGB)";
SmoothLcdBgr => "LCD (ClearType, BGR)", "LCD (ClearType, BGR)";
TextOutlineWidth => "Контур:", "Outline:";
TextOutlineInside => "Внутри", "Inside";
TextOutlineCenter => "По центру", "Center";
TextOutlineOutside => "Снаружи", "Outside";
SettingsInterface => "Интерфейс", "Interface";
GroupLanguageWindows => "Язык и окна", "Language and windows";
GroupSettingsFile => "Файл настроек", "Settings file";
GroupPanels => "Панели", "Panels";
    WindowLayout => "Расположение окон", "Window layout";
    GroupApp => "Программа", "Application";
    NewBackgroundColor => "Цветной фон", "Color background";
    NewBackgroundTransparent => "Прозрачный фон", "Transparent background";
    AboutDetails => "Версия и авторы", "Version and credits";
    ThemesCustom => "Свои темы", "Custom themes";
LayerQuickSettings => "Режим и непрозрачность в панели", "Blend and opacity in the panel";
LayerDoubleClick => "Двойной клик по слою", "Double-click on a layer";
LayerDoubleClickRename => "Переименовать", "Rename";
GroupAppearance => "Внешний вид", "Appearance";
ExportSettings => "Экспорт...", "Export...";
ImportSettings => "Импорт...", "Import...";
SettingsExported => "Настройки сохранены в файл.", "Settings saved to a file.";
SettingsImported => "Настройки загружены.", "Settings loaded.";
SettingsInvalid => "Это не файл настроек Chequer.", "This is not a Chequer settings file.";
Reset => "Сбросить", "Reset";
ResetTab => "Сбросить вкладку", "Reset tab";
ResetAll => "Сбросить всё", "Reset all";
ResetTabConfirm => "Вернуть настройки этой вкладки к значениям по умолчанию?", "Restore this tab's settings to their defaults?";
ResetAllConfirm => "Вернуть все настройки приложения к значениям по умолчанию?", "Restore all application settings to their defaults?";
ResetThemeConfirm => "Сбросить параметры и цвета этой темы?", "Reset this theme's options and colors?";
    ToolRectSelect => "Выбрать прямоугольник", "Rectangle Select";
    ToolEllipseSelect => "Выбрать овал", "Ellipse Select";
    ToolLasso => "Лассо", "Lasso Select";
    ToolMagicWand => "Волшебная палочка", "Magic Wand";
    ToolMovePixels => "Переместить выделенные пиксели", "Move Selected Pixels";
    ToolMoveSelection => "Переместить выделенную область", "Move Selection";
    ToolZoom => "Масштаб", "Zoom";
    ToolPan => "Рука", "Pan";
    ToolBucket => "Заливка", "Paint Bucket";
    ToolGradient => "Градиент", "Gradient";
    ToolCloneStamp => "Штамп клонирования", "Clone Stamp";
    ToolRecolor => "Перекраска", "Recolor";
    ToolText => "Текст", "Text";
    ToolBrush => "Кисть", "Paintbrush";
    ToolEraser => "Ластик", "Eraser";
    ToolPencil => "Карандаш", "Pencil";
    ToolPicker => "Пипетка", "Color Picker";
    ToolLine => "Линия", "Line";
    ToolShapes => "Фигуры", "Shapes";
    HintRectSelect => "Выделить область", "Select a region";
    HintLasso => "Обвести область", "Draw a selection";
    HintWand => "Выделить по цвету", "Select by color";
    HintMovePixels => "Переместить пиксели", "Move pixels";
    HintMoveSelection => "Переместить выделение", "Move selection";
    HintZoom => "Масштаб", "Zoom";
    HintPan => "Сдвинуть изображение", "Pan the image";
    HintBucket => "Залить область", "Fill an area";
    HintGradient => "Нарисовать градиент", "Draw a gradient";
    HintBrush => "Рисовать кистью", "Paint with the brush";
    HintEraser => "Стереть", "Erase";
    HintCloneStamp => "Ctrl+клик - задать источник, затем рисовать копией", "Ctrl+click to set the source, then paint the copy";
    HintPencil => "Рисовать карандашом", "Draw with the pencil";
    HintPicker => "Взять цвет", "Pick a color";
    HintLine => "Нарисовать линию; на кривой клик по линии добавляет точку", "Draw a line; in curve mode click the line to add a point";
    HintShapes => "Нарисовать фигуру", "Draw a shape";
    OptWidth => "Ширина:", "Width:";
    OptBrush => "Кисть:", "Brush:";
    BrushDialogTitle => "Кисти", "Brushes";
    BrushSpacing => "Интервал:", "Spacing:";
    BrushAngle => "Угол:", "Angle:";
    BrushRoundness => "Округлость:", "Roundness:";
    BrushAngleJitter => "Разброс угла:", "Angle jitter:";
    BrushSizeJitter => "Разброс размера:", "Size jitter:";
    BrushOpacityJitter => "Разброс непрозрачности:", "Opacity jitter:";
    BrushScatter => "Разброс позиции:", "Scatter:";
    BrushFlow => "Поток:", "Flow:";
    BrushFollowDirection => "Поворот по направлению штриха", "Rotate along the stroke";
    BrushTabLayers => "Слои следа", "Track layers";
    BrushLayerAdd => "Добавить слой", "Add a layer";
    BrushLayerFromImage => "Из картинки...", "From an image...";
    BrushLayerFromMarks => "Из меток (пунктир, точки, волна)", "From marks (dashes, dots, wave)";
    BrushMark => "Метка", "Mark";
    BrushMarkAdd => "Добавить метку", "Add a mark";
    BrushMarkRemove => "Убрать метку", "Remove mark";
    BrushMarkStart => "Начало:", "Start:";
    BrushHintMarkStart => "Где метка начинается внутри периода, в размерах кисти", "Where the mark begins inside the period, in brush sizes";
    BrushMarkLength => "Длина:", "Length:";
    BrushHintMarkLength => "Длина метки вдоль штриха, в размерах кисти", "Mark length along the stroke, in brush sizes";
    BrushMarkOffset => "Сдвиг поперёк:", "Offset across:";
    BrushHintMarkOffset => "Сдвиг от середины штриха, в размерах кисти", "Shift from the middle of the stroke, in brush sizes";
    BrushMarkThickness => "Толщина:", "Thickness:";
    BrushHintMarkThickness => "Толщина метки поперёк штриха; 1 - на всю ширину", "Mark thickness across the stroke; 1 is the full width";
    BrushMarkRounding => "Скругление:", "Rounding:";
    BrushHintMarkRounding => "От острых углов до полностью круглых концов", "From sharp corners to fully round ends";
    BrushMarkSway => "Размах волны:", "Wave swing:";
    BrushHintMarkSway => "Насколько метка уходит в стороны от середины штриха; 0 - прямая", "How far the mark swings aside from the middle of the stroke; 0 is straight";
    BrushMarkSwayCycles => "Волн на метку:", "Waves per mark:";
    BrushHintMarkSwayCycles => "Сколько полных волн укладывается в длину метки", "How many full waves fit in the mark length";
    BrushLayerReplace => "Заменить...", "Replace...";
    BrushLayerRemove => "Убрать слой", "Remove layer";
    BrushLayerPitch => "Длина повтора:", "Repeat length:";
    BrushHintLayerPitch => "Сколько размеров кисти занимает одно повторение картинки вдоль штриха", "How many brush sizes one repeat of the picture takes along the stroke";
    BrushLayerAmount => "Сила слоя:", "Layer strength:";
    BrushHintLayerAmount => "Насколько слой гасит краску", "How much the layer holds back the paint";
    BrushLayerPitches => "Чередование длин:", "Length sequence:";
    BrushHintLayerPitches => "Множители длины повтора по кругу, например 1 1.2 0.8: так блоки протектора не повторяются одинаково", "Repeat length multipliers used in turn, e.g. 1 1.2 0.8: tread blocks stop repeating identically";
    BrushGenerate => "Сгенерировать наконечник", "Generate a tip";
    BrushGeneratedTipHint => "Выберите сгенерированный наконечник, чтобы править его параметры", "Select a generated tip to edit its parameters";
    BrushCycleTips => "Наконечники по очереди", "Tips in turn";
    BrushAccumulate => "Накапливать краску при перекрытии", "Build up paint on overlap";
    BrushReset => "Сбросить", "Reset";
    BrushPresets => "Пресеты", "Presets";
    PresetDefault => "По умолчанию", "Default";
    PresetPen => "Перо", "Pen";
    PresetAirbrush => "Аэрограф", "Airbrush";
    PresetCalligraphy => "Каллиграфия", "Calligraphy";
    PresetWatercolor => "Акварель", "Watercolor";
    PresetScatter => "Россыпь", "Scatter";
    PresetStamp => "Штамп", "Stamp";
PresetRibbon => "Лента (перо)", "Ribbon (pen)";
    BrushSectionShape => "Форма", "Shape";
    BrushSectionRandom => "Случайность", "Randomness";
    BrushSectionPaint => "Краска", "Paint";
    BrushSearchHint => "Поиск кисти", "Search brushes";
    BrushCategoryBasic => "Базовые", "Basic";
    BrushCategorySoft => "Мягкие", "Soft";
    BrushCategoryTexture => "Текстурные", "Textured";
    BrushCategoryDecor => "Декоративные", "Decorative";
    BrushCategoryTracks => "Следы", "Tracks";
    BrushCategoryMine => "Мои кисти", "My brushes";
BrushCategoryFavorites => "Избранное", "Favorites";
BrushFavoriteAdd => "В избранное", "Add to favorites";
BrushFavoriteRemove => "Убрать из избранного", "Remove from favorites";
    BrushNothingFound => "Ничего не найдено", "Nothing found";
    Share => "Поделиться...", "Share...";
VolumeUndo => "Громкость: отмена и повтор", "Volume keys undo/redo";
PenOnly => "Рисовать только пером", "Pen-only drawing";
Haptics => "Вибрация", "Vibration";
PenWayland => "Нажим через Wayland", "Wayland pressure";
    PenWaylandHint => "Выключение - после перезапуска", "Turning off needs a restart";
    GroupPen => "Перо", "Pen";
    PenCurve => "Кривая", "Curve";
    PenDeadZone => "Мёртвая зона, %", "Dead zone, %";
    PenSmoothing => "Сглаживание, %", "Smoothing, %";
    PenMinPressure => "Мин. нажим, %", "Min pressure, %";
    BrushPressureRoundness => "Нажим меняет форму:", "Pressure affects roundness:";
    BrushPressureScatter => "Лёгкий нажим разбрасывает:", "Light pressure scatters:";
    BrushPressureHardness => "Нажим меняет жёсткость края:", "Pressure affects edge hardness:";
    BrushPressureSpacing => "Лёгкий нажим раздвигает штампы:", "Light pressure widens spacing:";
    BrushTiltAngle => "Наклон поворачивает штамп:", "Tilt rotates the tip:";
    BrushTiltRoundness => "Наклон сплющивает штамп:", "Tilt flattens the tip:";
    BrushSpeedSize => "Скорость уменьшает размер:", "Speed shrinks size:";
    BrushSpeedOpacity => "Скорость уменьшает плотность:", "Speed lowers opacity:";
    BrushHueJitter => "Разброс оттенка:", "Hue jitter:";
    BrushSaturationJitter => "Разброс насыщенности:", "Saturation jitter:";
    BrushValueJitter => "Разброс яркости:", "Value jitter:";
    BrushPressureSize => "Нажим меняет размер:", "Pressure affects size:";
    BrushPressureOpacity => "Нажим меняет плотность:", "Pressure affects opacity:";
    BrushPressureSizeCurve => "Кривая нажима на размер:", "Pressure to size curve:";
    BrushPressureOpacityCurve => "Кривая нажима на плотность:", "Pressure to opacity curve:";
    BrushImport => "Импорт...", "Import...";
    BrushExport => "Экспорт...", "Export...";
    BrushDelete => "Удалить", "Delete";
Yes => "Да", "Yes";
No => "Нет", "No";
    BrushNew => "Новая кисть", "New brush";
    BrushEdit => "Редактировать", "Edit";
    BrushSave => "Сохранить", "Save";
    BrushSectionTips => "Изображения кисти", "Brush images";
    BrushAddImages => "Добавить изображения...", "Add images...";
    BrushTipRemove => "Убрать изображение", "Remove image";
    BrushDeleteQuestion => "Удалить кисть", "Delete brush";
    BrushDeleteLocked => "Встроенные кисти удалить нельзя", "Built-in brushes cannot be deleted";
    BrushNameTaken => "Кисть с таким названием уже есть", "A brush with this name already exists";
    BrushTipShare => "Доля штампов:", "Share of dabs:";
    BrushNameHint => "Имя кисти", "Brush name";
    BrushDuplicate => "Дублировать", "Duplicate";
    BrushCopySuffix => "копия", "copy";
    BrushResetHint => "Вернуть исходные настройки встроенной кисти", "Restore the built-in brush settings";
    BrushResetQuestion => "Вернуть кисть к исходному виду? Ваши изменения пропадут", "Restore the original brush? Your changes will be lost";
    BrushTabStroke => "Штрих", "Stroke";
    BrushTabDynamics => "Динамика", "Dynamics";
    BrushMore => "Ещё", "More";
    BrushHintHardness => "Насколько резкий край у отпечатка: 100% — чёткий, меньше — мягкий", "How sharp the tip edge is: 100% is crisp, lower is soft";
    BrushHintRoundness => "Сплющивание кончика: 100% — круг, меньше — овал", "Squashes the tip: 100% is round, lower is oval";
    BrushHintAngle => "Поворот кончика в градусах", "Rotation of the tip in degrees";
    BrushHintFollowDirection => "Кончик разворачивается вслед за движением пера", "The tip turns to follow the pen direction";
    BrushHintSpacing => "Расстояние между отпечатками, % от размера кисти. Меньше — сплошная линия, больше — пунктир", "Distance between dabs, % of brush size. Lower is a solid line, higher is dotted";
    BrushHintScatter => "Как далеко отпечатки разлетаются от линии штриха", "How far dabs fly away from the stroke line";
    BrushHintSizeStart => "Размер кисти в начале штриха, % от обычного", "Brush size at the start of the stroke, % of normal";
    BrushHintSizeEnd => "Размер кисти в конце штриха, % от обычного", "Brush size at the end of the stroke, % of normal";
    BrushHintSizeFade => "На какой длине штрих переходит от начального размера к конечному, в размерах кисти", "Stroke length over which the size goes from start to end, in brush sizes";
    BrushHintPressureSize => "Насколько нажим меняет размер. 0% — размер не зависит от нажима", "How much pressure changes the size. 0% means size ignores pressure";
    BrushHintPressureSizeCurve => "Как размер растёт с силой нажима", "How size grows with pen pressure";
    BrushHintPressureOpacity => "Насколько нажим меняет плотность. 0% — плотность не зависит от нажима", "How much pressure changes opacity. 0% means opacity ignores pressure";
    BrushHintPressureOpacityCurve => "Как плотность растёт с силой нажима", "How opacity grows with pen pressure";
    BrushHintSpeedSize => "Чем быстрее движение, тем меньше размер", "The faster you move, the smaller the size";
    BrushHintSpeedOpacity => "Чем быстрее движение, тем прозрачнее мазок", "The faster you move, the more transparent the stroke";
    BrushHintPressureRoundness => "Нажим меняет округлость кончика", "Pressure changes the tip roundness";
    BrushHintPressureScatter => "Чем легче нажим, тем сильнее разлёт отпечатков", "The lighter the pressure, the wider the dabs scatter";
    BrushHintPressureHardness => "Нажим меняет мягкость края", "Pressure changes the edge softness";
    BrushHintPressureSpacing => "Чем легче нажим, тем реже отпечатки", "The lighter the pressure, the sparser the dabs";
    BrushHintTiltAngle => "Наклон пера поворачивает кончик (нужно перо с датчиком наклона)", "Pen tilt rotates the tip (needs a pen with a tilt sensor)";
    BrushHintTiltRoundness => "Наклон пера сплющивает кончик (нужно перо с датчиком наклона)", "Pen tilt flattens the tip (needs a pen with a tilt sensor)";
    BrushHintSizeJitter => "Случайный размер каждого отпечатка", "Random size for every dab";
    BrushHintAngleJitter => "Случайный поворот каждого отпечатка", "Random rotation for every dab";
    BrushHintOpacityJitter => "Случайная плотность каждого отпечатка", "Random opacity for every dab";
    BrushHintHueJitter => "Случайный сдвиг оттенка на каждом отпечатке", "Random hue shift on every dab";
    BrushHintSaturationJitter => "Случайный сдвиг насыщенности на каждом отпечатке", "Random saturation shift on every dab";
    BrushHintValueJitter => "Случайный сдвиг яркости на каждом отпечатке", "Random brightness shift on every dab";
    BrushHintFlow => "Сколько краски ложится за один отпечаток. Меньше — прозрачнее, штрих набирается медленнее", "Paint laid down per dab. Lower is more transparent and builds up slower";
    BrushHintAccumulate => "Выключено: перекрытия отпечатков не темнеют. Включено: краска копится, как у аэрографа", "Off: overlapping dabs do not darken. On: paint builds up like an airbrush";
    BrushHintGrain => "Насколько зерно бумаги проступает сквозь мазок", "How much the paper grain shows through the stroke";
    BrushHintGrainScale => "Размер зерна бумаги", "Size of the paper grain";
    BrushHintPaper => "Узор бумаги, из которого берётся зерно", "Paper pattern the grain comes from";
    BrushHintDryness => "Мазок становится сухим: краска ложится с пропусками", "The stroke gets dry: paint lays down with gaps";
    BrushHintDrynessSpeed => "Чем быстрее движение, тем суше мазок", "The faster you move, the drier the stroke";
    BrushHintDrynessPressure => "Чем легче нажим, тем суше мазок", "The lighter the pressure, the drier the stroke";
    BrushHintDepletion => "Краска постепенно заканчивается по ходу штриха", "Paint gradually runs out along the stroke";
    BrushHintCycleTips => "Наконечники набора идут строго по порядку, а не случайно: левая и правая нога, чередование следов", "Tips of the set go strictly in order instead of randomly: left and right foot, alternating prints";
    BrushHintBristles => "Полосы от отдельных щетинок вдоль штриха", "Streaks from individual bristles along the stroke";
    BrushHintHairs => "Расположение щетинок в кисти", "How the bristles are laid out in the brush";
    BrushHintTextureScale => "Размер деталей узора", "Size of the pattern details";
    BrushHintTextureThickness => "Толщина отдельных щетинок", "Thickness of the individual bristles";
    BrushHintTextureDensity => "Сколько деталей на единицу площади", "How many details per area";
    BrushHintTextureContrast => "Разница между светлым и тёмным в узоре", "Difference between light and dark in the pattern";
    BrushHintTextureUneven => "Насколько щетинки отличаются друг от друга", "How different the bristles are from each other";
    BrushDiscardTitle => "Несохранённые правки", "Unsaved changes";
    BrushDiscardQuestion => "В кисти есть несохранённые правки. Закрыть без сохранения?", "The brush has unsaved changes. Discard them?";
    BrushUnnamed => "Без имени", "Unnamed";
    BrushImportUnsupported => "Этот тип файла нельзя импортировать как кисть", "This file type cannot be imported as a brush";
    BrushNoLibraryDir => "Не найдена папка для кистей", "Brush folder not found";
    BrushCategoryPaint => "Классические", "Classic";
    BrushHardness => "Жёсткость:", "Hardness:";
    BrushSizeStart => "Размер в начале:", "Size at start:";
    BrushSizeEnd => "Размер в конце:", "Size at end:";
    BrushSizeFade => "Длина перехода (×размер):", "Transition (× size):";
    BrushCategoryDrawing => "Рисунок", "Drawing";
    BrushCategoryLines => "Линии", "Lines";
    BrushCategoryPainting => "Живопись", "Painting";
    BrushGrain => "Сила зерна:", "Grain strength:";
    BrushGrainScale => "Размер зерна:", "Grain size:";
    BrushTexture => "Текстура:", "Texture:";
    BrushHairs => "Узор щетины:", "Bristle pattern:";
    BrushTextureBuiltin => "Встроенная", "Built-in";
    BrushTextureFile => "Из файла…", "From file…";
    TextureNoise => "Шум", "Noise";
    TextureFibers => "Волокна", "Fibers";
    TextureDots => "Точки", "Dots";
    TextureCells => "Ячейки", "Cells";
    TextureWeave => "Холст", "Canvas";
    TextureBristles => "Щетина", "Bristles";
    TextureScale => "Размер узора:", "Pattern size:";
    TextureThickness => "Толщина щетинок:", "Bristle thickness:";
    TextureDensity => "Густота:", "Density:";
    TextureContrast => "Контраст:", "Contrast:";
    TextureUneven => "Неровность:", "Unevenness:";
    TextureReseed => "Другой узор", "New variation";
    BrushEngineStamp => "Штамповая", "Stamp";
    BrushEngineRibbon => "Лента", "Ribbon";
    BrushEngineHint => "Тип кисти", "Brush type";
    BrushDryness => "Сухость:", "Dryness:";
    BrushDrynessSpeed => "Скорость сушит:", "Speed dries:";
    BrushDrynessPressure => "Лёгкий нажим сушит:", "Light pressure dries:";
    BrushBristles => "Щетина (полосы):", "Bristles (streaks):";
    BrushDepletion => "Краска кончается к концу штриха:", "Paint runs out along the stroke:";
    OptFill => "Заполнение:", "Fill:";
    OptTolerance => "Чувствительность:", "Tolerance:";
    OptPickArea => "Область:", "Area:";
PickMixLinear => "Линейное смешение", "Linear mix";
PickMixSrgb => "Смешение sRGB", "sRGB mix";
PickAlphaWeighted => "Альфа: по весу", "Alpha: weighted";
PickAlphaPlain => "Альфа: среднее", "Alpha: plain mean";
PickAlphaOpaque => "Альфа: без прозрачности", "Alpha: opaque";
OptSampling => "Выборка:", "Sampling:";
    OptFlood => "Заливка:", "Flood mode:";
    OptGradient => "Градиент:", "Gradient:";
    GradientLinear => "Линейный", "Linear";
    GradientReflected => "Линейный отражённый", "Linear reflected";
    GradientDiamond => "Ромбовидный", "Diamond";
    GradientRadial => "Радиальный", "Radial";
    GradientConical => "Конический", "Conical";
    OptShape => "Фигура:", "Shape:";
    OptMode => "Режим:", "Mode:";
OptResample => "Масштаб:", "Resample:";
    OptPixelPerfect => "Идеальные пиксели", "Pixel perfect";
    OptSmoothPath => "Сглаживание траектории", "Smooth path";
    OptAntialias => "Сглаживание", "Antialiasing";
    OptDone => "Готово", "Finish";
    SamplingLayer => "Слой", "Layer";
    SamplingImage => "Изображение", "Image";
    FloodContiguous => "Смежные", "Contiguous";
    FloodGlobal => "Глобально", "Global";
    FillOutline => "Только контур", "Outline";
    FillFilled => "Только заливка", "Filled";
    FillBoth => "Контур и заливка", "Outline and fill";
    ModeReplace => "Заменить", "Replace";
    ModeAdd => "Добавить", "Add";
    ModeSubtract => "Вычесть", "Subtract";
    ModeIntersect => "Пересечение", "Intersect";
    ModeXor => "Исключающее ИЛИ", "Invert";
    PasteBlend => "Смешивание", "Blend";
    PasteReplace => "Замена", "Replace";
    BlendNormal => "Нормальный", "Normal";
    BlendMultiply => "Умножение", "Multiply";
    BlendScreen => "Экран", "Screen";
    BlendOverlay => "Перекрытие", "Overlay";
    BlendDarken => "Затемнение", "Darken";
    BlendLighten => "Осветление", "Lighten";
    BlendDifference => "Разница", "Difference";
    BlendAdditive => "Сложение", "Additive";
    BlendColorBurn => "Затемнение основы", "Color Burn";
    BlendColorDodge => "Осветление основы", "Color Dodge";
    BlendReflect => "Отражение", "Reflect";
    BlendGlow => "Свечение", "Glow";
    BlendNegation => "Отрицание", "Negation";
    BlendXor => "Исключающее ИЛИ", "XOR";
    BlendHardLight => "Жёсткий свет", "Hard Light";
    BlendSoftLight => "Мягкий свет", "Soft Light";
    BlendColor => "Цвет", "Color";
    BlendLuminosity => "Яркость", "Luminosity";
    BlendHue => "Оттенок", "Hue";
    BlendSaturation => "Насыщенность", "Saturation";
    ShapeRectangle => "Прямоугольник", "Rectangle";
    ShapeRounded => "Скруглённый прямоугольник", "Rounded Rectangle";
    ShapeEllipse => "Эллипс", "Ellipse";
    ShapeTriangle => "Треугольник", "Triangle";
    ShapeDiamond => "Ромб", "Diamond";
    ShapeStar => "Звезда", "Star";
    ShapePolygon => "Многоугольник", "Polygon";
    OptCornerRadius => "Скругление:", "Corner radius:";
    OptApex => "Вершина:", "Apex:";
    OptSides => "Сторон:", "Sides:";
    OptPoints => "Лучей:", "Points:";
    OptInnerRadius => "Внутр. радиус:", "Inner radius:";
    OptOuterRadius => "Внешн. радиус:", "Outer radius:";
    OptArrowHead => "Наконечник:", "Head:";
    OptArrowShaft => "Стержень:", "Shaft:";
    ShapeArrow => "Стрелка", "Arrow";
    ShapeHeart => "Сердце", "Heart";
    Background => "Фон", "Background";
    LayerN => "Слой", "Layer";
    LayerName => "Имя", "Name";
    LayerOpacity => "Непрозрачность", "Opacity";
    LayerBlendMode => "Режим смешивания:", "Blend mode:";
    LayerVisible => "Видимый", "Visible";
    PickerLayout => "Вид окна цвета", "Color window layout";
    LayerMenuAddMasked => "Новый слой под слоем (в маске)", "New layer below (masked)";
    PaletteAltDelete => "Alt+клик: удалить", "Alt+click: remove";
    DropNestInto => "Вложить в", "Nest into";
    LayerMenuRelease => "Вынуть из маски", "Release from mask";
    PickerMore => "Ещё...", "More...";
    GroupColorPicker => "Выбор цвета", "Color picker";
    ColorPickerTitle => "Выбор цвета", "Color picker";
    ColorPickerCustomize => "Настроить...", "Customize...";
    ColorPickerPreview => "Предпросмотр", "Preview";
    HighContrastIcons => "Контрастные иконки (200%)", "High contrast icons (200%)";
    CompactColors => "Под инструментами", "Under tools";
    PaletteBelow => "Палитра внутри", "Palette inside";
    SwapColors => "Поменять цвета", "Swap colors";
    ResetColors => "Чёрный и белый", "Black and white";
    PresetCompact => "Компактный", "Compact";
    PresetFull => "Полный", "Full";
    SlidersBeside => "Ползунки сбоку", "Sliders beside";
SelectorWheel => "Колесо", "Wheel";
    SelectorSquare => "Квадрат", "Square";
    SelectorNone => "Нет", "None";
    ShowRgb => "RGB", "RGB";
    ShowHsv => "HSV", "HSV";
    ShowAlpha => "Альфа", "Alpha";
    ShowHex => "Hex", "Hex";
    SwatchSize => "Размер ячеек палитры", "Palette swatch size";
    SizeSmall => "Мелкие", "Small";
    SizeMedium => "Средние", "Medium";
    SizeLarge => "Крупные", "Large";
    PaletteAddColor => "Добавить текущий цвет в палитру", "Add current color to palette";
    PaletteRemoveColor => "Убрать текущий цвет из палитры", "Remove current color from palette";
    PaletteMenu => "Палитры", "Palettes";
    PaletteLoad => "Загрузить из файла...", "Load from file...";
    PaletteSave => "Сохранить в файл...", "Save to file...";
    PaletteFromImage => "Взять из изображения", "Take from image";
    PaletteReset => "Стандартная палитра", "Default palette";
    PaletteLoaded => "Палитра загружена.", "Palette loaded.";
    PaletteSaved => "Палитра сохранена.", "Palette saved.";
    PaletteNoColors => "В изображении нет непрозрачных пикселей.", "The image has no opaque pixels.";
    Hex => "Hex:", "Hex:";
    HistoryOpened => "Открыто", "Opened";
    HNew => "Создано изображение", "New Image";
    HPencil => "Карандаш", "Pencil";
    HBrush => "Кисть", "Paintbrush";
    HEraser => "Ластик", "Eraser";
    HCloneStamp => "Штамп клонирования", "Clone Stamp";
    HFill => "Заливка", "Paint Bucket";
    HGradient => "Градиент", "Gradient";
    HLine => "Линия", "Line";
    HRectangle => "Прямоугольник", "Rectangle";
    HRounded => "Скруглённый прямоугольник", "Rounded Rectangle";
    HEllipse => "Эллипс", "Ellipse";
    HFreeform => "Свободная фигура", "Freeform Shape";
    HShape => "Фигура", "Shape";
    HSelectRect => "Выбрать прямоугольник", "Rectangle Select";
    HSelectEllipse => "Выбрать овал", "Ellipse Select";
    HSelectLasso => "Лассо", "Lasso Select";
    HSelectWand => "Волшебная палочка", "Magic Wand";
    HSelectAll => "Выделить все", "Select All";
    HDeselect => "Отменить выделение", "Deselect";
    HInvertSelection => "Обратить выделение", "Invert Selection";
    HPasteSelection => "Вставка выделения", "Paste Selection";
    HMoveSelection => "Перемещение выделения", "Move Selection";
    HModifySelection => "Изменение выделения", "Modify Selection";
    HMovePixels => "Перемещение пикселей", "Move Selected Pixels";
    HPaste => "Вставка", "Paste";
    HCut => "Вырезать", "Cut";
    HErase => "Очистить выделение", "Erase Selection";
    HFillSelection => "Заполнить выделение", "Fill Selection";
    HAddLayer => "Добавить слой", "Add New Layer";
    HDuplicateLayer => "Дублировать слой", "Duplicate Layer";
    HDeleteLayer => "Удалить слой", "Delete Layer";
    HMergeLayer => "Слить слои", "Merge Layer Down";
    HMoveLayerUp => "Поднять слой", "Move Layer Up";
    HMoveLayerDown => "Опустить слой", "Move Layer Down";
    HMaskLayer => "Вложить слой в маску", "Nest Layer under Mask";
    HUnmaskLayer => "Вынуть слой из маски", "Release Layer from Mask";
    HLayerProps => "Свойства слоя", "Layer Properties";
    HCrop => "Обрезать по выделению", "Crop to Selection";
    HResize => "Изменить размер", "Resize Image";
    HCanvasSize => "Размер полотна", "Canvas Size";
    HFlipH => "Отражение по горизонтали", "Flip Horizontal";
HOffsetHalf => "Сдвиг со склейкой", "Wrapping Offset";
    HFlipV => "Отражение по вертикали", "Flip Vertical";
    HRotateCw => "Поворот на 90° по часовой", "Rotate 90° Clockwise";
    HRotateCcw => "Поворот на 90° против часовой", "Rotate 90° Counter-Clockwise";
    HRotate180 => "Поворот на 180°", "Rotate 180°";
    HFlatten => "Объединить слои", "Flatten";
    HAddFrame => "Новый кадр", "New Frame";
    HDuplicateFrame => "Дубль кадра", "Duplicate Frame";
    HDeleteFrame => "Удаление кадра", "Delete Frame";
    HMoveFrame => "Перемещение кадра", "Move Frame";
    HFrameDuration => "Длительность кадра", "Frame Duration";
    HClearCels => "Очистка ячеек", "Clear Cels";
    HPasteCels => "Вставка ячеек", "Paste Cels";
    HReverseFrames => "Разворот кадров", "Reverse Frames";
    HInvertColors => "Инвертировать цвета", "Invert Colors";
    HBlackWhite => "Чёрно-белое", "Black and White";
    Ok => "OK", "OK";
    Cancel => "Отмена", "Cancel";
    Width => "Ширина", "Width";
    Height => "Высота", "Height";
    KeepAspect => "Сохранять пропорции", "Maintain aspect ratio";
    MemoryEstimate => "Размер в памяти", "Memory estimate";
    Resampling => "Передискретизация", "Resampling";
    ResampleNearest => "Ближайший сосед", "Nearest neighbor";
    ResampleBilinear => "Билинейная", "Bilinear";
    ResampleBicubic => "Бикубическая", "Bicubic";
    ResampleLanczos => "Ланцош", "Lanczos";
    Anchor => "Привязка", "Anchor";
    SaveConfig => "Параметры сохранения", "Save Configuration";
    FileFormat => "Формат файла", "File type";
    JpegQuality => "Качество", "Quality";
    BitDepth => "Глубина цвета", "Bit depth";
    PngAuto => "Авто (самый маленький файл без потерь)", "Auto-detect (smallest lossless)";
    Png32 => "32 бита (RGBA)", "32-bit (RGBA)";
    Png24 => "24 бита (RGB, без прозрачности)", "24-bit (RGB, no transparency)";
    Compression => "Сжатие", "Compression";
    CompressFast => "Быстро", "Fast";
    CompressDefault => "Обычное", "Default";
    CompressBest => "Максимальное", "Best";
    WithAlpha => "С прозрачностью (32 бита)", "With transparency (32-bit)";
    FileSize => "Размер файла", "File size";
    NoOptions => "У этого формата нет настроек.", "This format has no options.";
    SaveHistory => "Сохранить историю", "Save history";
    WebpNote => "WebP сохраняется без потерь.", "WebP is saved losslessly.";
    FlattenTitle => "Сохранение без слоёв", "Saving without layers";
    FlattenBody => "Этот формат не поддерживает слои. В файл попадёт одно объединённое изображение, слои сохранятся только в .chq или .ora. Документ в редакторе останется слойным.", "This format has no layers. The file will contain a single merged image; layers are kept only in .chq or .ora. The document in the editor stays layered.";
    PasteTooBig => "Вставка больше холста", "The paste is larger than the canvas";
    PasteImageSize => "Изображение", "Image";
    PasteCanvasSize => "Холст", "Canvas";
    PasteAsIs => "Вставить как есть", "Paste as is";
    PasteAsIsHint => "Холст не меняется", "The canvas stays as is";
    PasteExpand => "Расширить холст", "Expand the canvas";
    PasteExpandHint => "Холст станет по размеру вставки", "The canvas grows to fit";
    DropTitle => "Добавить изображение", "Add image";
    DropFilesCount => "Файлов", "Files";
    DropOpenTab => "Открыть в новой вкладке", "Open in a new tab";
    DropOpenTabHint => "Отдельный документ", "A separate document";
    DropAddLayer => "Добавить как новый слой", "Add as a new layer";
    DropAddLayerHint => "Слой в текущем документе", "A layer in this document";
    ReopenTab => "Восстановить закрытую вкладку", "Reopen closed tab";
    UnsavedTitle => "Несохранённые изменения", "Unsaved changes";
    UnsavedBody => "Сохранить изменения перед закрытием?", "Save changes before closing?";
    DontSave => "Не сохранять", "Don't save";
    Untitled => "Безымянный", "Untitled";
    AboutBody => "Chequer Image Studio - быстрый растровый редактор.\nБета 0.5.0, лицензия GPL-3.0.", "Chequer Image Studio - a fast raster editor.\nBeta 0.5.0, GPL-3.0 license.";
    AboutIcons => "Шрифт: Aseprite Font © David Capello, CC BY 4.0 (изменён: добавлены знаки «Ё», «Й»).", "Font: Aseprite Font © David Capello, CC BY 4.0 (modified: added marks for Ё, Й).";
    ErrorTitle => "Ошибка", "Error";
    SoonHint => "В разработке...", "In development...";
    Saved => "Сохранено", "Saved";
RecoveredName => "Восстановлено", "Recovered";
RecoveredStatus => "Восстановлено после сбоя:", "Recovered after a crash:";
    ClipboardEmpty => "В буфере обмена нет изображения.", "There is no image on the clipboard.";
    TextAlignLeftShort => "Л", "L";
    TextAlignCenterShort => "Ц", "C";
    TextAlignRightShort => "П", "R";
    FxAdjustments => "Корректировки", "Adjustments";
    FxPhoto => "Для фотографий", "Photo";
    FxDistort => "Искажение", "Distort";
    FxObject => "Объект", "Object";
    FxBlurs => "Размытие", "Blurs";
    FxStylize => "Стилизация", "Stylize";
    FxRender => "Узоры", "Render";
    FxArtistic => "Художественные", "Artistic";
    FxColor => "Цвет", "Color";
    FxNoise => "Шум", "Noise";
    FxGlitch => "Глитч", "Glitch";
    FxRepeat => "Повторить", "Repeat";
    FxReset => "Сбросить", "Reset";
    FxRandomize => "Случайно", "Randomize";
}

#[cfg(test)]
mod tests {
    use super::*;

    static PLUGIN_TEXTS: &[TextEntry] = &[TextEntry::new("effect.test_plugin_effect", "Пример", "Example")];
    static CLASHING_TEXTS: &[TextEntry] = &[TextEntry::new("effect.test_plugin_effect", "Другое", "Other")];

    #[test]
    fn registered_text_resolves_and_unknown_falls_back_to_name() {
        assert_eq!(register_texts(PLUGIN_TEXTS), Ok(()));
        let shown = text_or_key(TextScope::Effect, "test_plugin_effect");
        assert!(shown == "Пример" || shown == "Example");
        assert_eq!(text_or_key(TextScope::Effect, "test_missing"), "test_missing");
        assert_eq!(register_texts(CLASHING_TEXTS), Err(DuplicateTextKey("effect.test_plugin_effect")));
    }
}

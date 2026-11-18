use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

const SOCKET_FILE: &str = "bridge.sock";
const END_MARK: &str = "END";

static BRIDGE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_dir(dir: PathBuf) {
    let _ = BRIDGE_DIR.set(dir);
}

pub fn dir() -> Option<&'static PathBuf> {
    BRIDGE_DIR.get()
}

pub fn request(line: &str) -> Result<Vec<String>, String> {
    request_within(line, None)
}

pub fn request_within(line: &str, timeout: Option<Duration>) -> Result<Vec<String>, String> {
    let dir = BRIDGE_DIR.get().ok_or("мост к системным службам не настроен")?;
    let mut stream = UnixStream::connect(dir.join(SOCKET_FILE)).map_err(|e| format!("мост к системным службам недоступен: {e}"))?;
    stream.set_read_timeout(timeout).map_err(|e| e.to_string())?;
    writeln!(stream, "{line}").map_err(|e| e.to_string())?;
    let lines = BufReader::new(stream).lines().map_while(Result::ok).take_while(|l| l != END_MARK).collect();
    Ok(lines)
}

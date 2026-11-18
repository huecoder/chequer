use std::fs::{self, File};
use std::path::{Path, PathBuf};

use chequer_core::chq::{WriteMode, decode_document, encode_document};
use chequer_core::document::{Document, DocumentPalette};

use crate::palette::Palette;
use crate::view::DocTab;

const LOCK_FILE: &str = ".lock";
const DOCUMENT_EXTENSION: &str = "chq";
const ORIGIN_EXTENSION: &str = "origin";

pub struct Recovered {
    pub document: Document,
    pub origin: Option<PathBuf>,
}

pub struct Session {
    dir: PathBuf,
    _lock: File,
}

fn open_lock(dir: &Path) -> Option<File> {
    File::options().create(true).write(true).truncate(false).open(dir.join(LOCK_FILE)).ok()
}

fn read_recovered(path: &Path) -> Option<Recovered> {
    let document = decode_document(&fs::read(path).ok()?).ok()?;
    let origin = fs::read_to_string(path.with_extension(ORIGIN_EXTENSION)).ok().filter(|text| !text.is_empty()).map(PathBuf::from);
    Some(Recovered { document, origin })
}

fn recover_abandoned_dir(dir: &Path) -> Option<Vec<Recovered>> {
    let mut files: Vec<PathBuf> = {
        let lock = open_lock(dir)?;
        lock.try_lock().ok()?;
        fs::read_dir(dir).ok()?.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == DOCUMENT_EXTENSION)).collect()
    };
    files.sort();
    let recovered = files.iter().filter_map(|path| read_recovered(path)).collect();
    let _ = fs::remove_dir_all(dir);
    Some(recovered)
}

pub fn restore_abandoned() -> Vec<Recovered> {
    let Some(root) = crate::config::recovery_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    dirs.sort();
    dirs.iter().filter_map(|dir| recover_abandoned_dir(dir)).flatten().collect()
}

impl Session {
    pub fn open() -> Option<Session> {
        let dir = crate::config::recovery_dir()?.join(std::process::id().to_string());
        fs::create_dir_all(&dir).ok()?;
        let lock = open_lock(&dir)?;
        lock.try_lock().ok()?;
        Some(Session { dir, _lock: lock })
    }

    fn document_path(&self, id: u64) -> PathBuf {
        self.dir.join(format!("tab-{id}.{DOCUMENT_EXTENSION}"))
    }

    pub fn forget(&self, id: u64) {
        let path = self.document_path(id);
        let _ = fs::remove_file(path.with_extension(ORIGIN_EXTENSION));
        let _ = fs::remove_file(path);
    }

    fn write(&self, tab: &mut DocTab, palette: &DocumentPalette) -> bool {
        tab.doc.palette = Some(palette.clone());
        let Ok(bytes) = encode_document(&tab.doc, WriteMode::Fast, false) else {
            return false;
        };
        let path = self.document_path(tab.id);
        let origin = tab.path.as_deref().map(|origin| origin.to_string_lossy().into_owned()).unwrap_or_default();
        fs::write(path.with_extension(ORIGIN_EXTENSION), origin).is_ok() && fs::write(path, bytes).is_ok()
    }

    pub fn sync(&self, tabs: &mut [DocTab], palette: &Palette) {
        let palette = DocumentPalette::from(palette);
        for tab in tabs {
            let revision = tab.doc.revision();
            if !tab.is_modified() {
                if tab.autosaved_revision.take().is_some() {
                    self.forget(tab.id);
                }
            } else if tab.autosaved_revision != Some(revision) && self.write(tab, &palette) {
                tab.autosaved_revision = Some(revision);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

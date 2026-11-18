use std::path::{Path, PathBuf};
use std::sync::Arc;

use chequer_core::brush::{Brush, FILE_EXTENSION, DEFAULT_PACK, IMPORT_EXTENSIONS, decode_foreign, decode_pack, encode_pack};

use crate::tools::ToolOptions;

const TIP_IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "bmp", "gif", "webp"];
const FALLBACK_FILE_STEM: &str = "brush";
const USER_KEY_PREFIX: &str = "user:";

pub struct BrushEntry {
    pub brush: Arc<Brush>,
    pub source: Option<PathBuf>,
    pub modified: bool,
}

pub struct BrushLibrary {
    entries: Vec<BrushEntry>,
    problems: Vec<String>,
    favorites: Vec<String>,
    overrides: Vec<Brush>,
}

fn brush_key(brush: &Brush) -> String {
    if brush.meta.id.is_empty() { format!("{USER_KEY_PREFIX}{}", brush.meta.name) } else { brush.meta.id.clone() }
}

impl BrushLibrary {
    pub fn load() -> Self {
        seed_defaults();
        let mut entries: Vec<BrushEntry> = Vec::new();
        let mut problems = Vec::new();
        for path in user_files() {
            match read_pack(&path) {
                Ok(brushes) => entries.extend(brushes.into_iter().map(|brush| BrushEntry { brush: Arc::new(brush), source: Some(path.clone()), modified: false })),
                Err(error) => problems.push(format!("{}: {error}", path.display())),
            }
        }
        if crate::config::brushes_dir().is_none() {
            entries.extend(default_brushes().into_iter().map(|brush| BrushEntry { brush: Arc::new(brush), source: None, modified: false }));
        }
        let overrides = load_overrides();
        for entry in &mut entries {
            if let Some(changed) = overrides.iter().find(|changed| !changed.meta.id.is_empty() && changed.meta.id == entry.brush.meta.id) {
                entry.brush = Arc::new(changed.clone());
                entry.modified = true;
            }
        }
        Self { entries, problems, favorites: crate::config::load_favorite_brushes(), overrides }
    }

    pub fn favorite_indices(&self) -> Vec<usize> {
        self.favorites.iter().filter_map(|key| self.entries.iter().position(|entry| brush_key(&entry.brush) == *key)).collect()
    }

    pub fn favorite_state(&self, brush: &Brush) -> Option<bool> {
        self.find(brush).map(|_| self.favorites.contains(&brush_key(brush)))
    }

    pub fn toggle_favorite(&mut self, brush: &Brush) {
        let key = brush_key(brush);
        match self.favorites.iter().position(|favorite| *favorite == key) {
            Some(position) => {
                self.favorites.remove(position);
            }
            None => self.favorites.push(key),
        }
        crate::config::save_favorite_brushes(&self.favorites);
    }

    pub fn move_favorite(&mut self, from: usize, to: usize) {
        let mut order = self.favorite_indices();
        if from >= order.len() || to >= order.len() {
            return;
        }
        let moved = order.remove(from);
        order.insert(to, moved);
        self.favorites = order.into_iter().map(|index| brush_key(&self.entries[index].brush)).collect();
        crate::config::save_favorite_brushes(&self.favorites);
    }

    pub fn entries(&self) -> &[BrushEntry] {
        &self.entries
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn find(&self, brush: &Brush) -> Option<usize> {
        let same = |other: &Brush| {
            if brush.meta.id.is_empty() {
                other.meta.id.is_empty() && other.meta.name == brush.meta.name
            } else {
                other.meta.id == brush.meta.id
            }
        };
        self.entries.iter().position(|entry| same(&entry.brush))
    }

    pub fn import(&mut self, path: &Path) -> Result<(), String> {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if extension == FILE_EXTENSION {
            let brushes = read_pack(path).map_err(|error| error.to_string())?;
            return self.store(&brushes, &file_stem(path));
        }
        if IMPORT_EXTENSIONS.contains(&extension.as_str()) {
            let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
            let brushes = decode_foreign(&extension, &file_stem(path), &bytes).map_err(|error| error.to_string())?;
            return self.store(&brushes, &file_stem(path));
        }
        if !TIP_IMAGE_EXTENSIONS.contains(&extension.as_str()) {
            return Err(crate::i18n::t(crate::i18n::Text::BrushImportUnsupported).to_owned());
        }
        let image = image::open(path).map_err(|error| error.to_string())?;
        let stem = file_stem(path);
        let brush = Brush::from_tip_image(&stem, &image).map_err(|error| error.to_string())?;
        self.store(&[brush], &stem)
    }

    pub fn save(&mut self, brush: Brush) -> Result<(), String> {
        let stem = file_stem(Path::new(&brush.meta.name));
        self.store(&[brush], &stem)
    }

    pub fn reset_builtin(&mut self, index: usize) -> Result<(), String> {
        let original = self
            .entries
            .get(index)
            .and_then(|entry| builtin_original(&entry.brush.meta.id))
            .ok_or_else(|| crate::i18n::t(crate::i18n::Text::BrushDeleteLocked).to_owned())?;
        self.override_builtin(index, original.clone(), &original)
    }

    pub fn replace(&mut self, index: usize, brush: Brush) -> Result<(), String> {
        if let Some(original) = builtin_original(&brush.meta.id) {
            return self.override_builtin(index, brush, &original);
        }
        let Some(path) = self.entries.get(index).and_then(|entry| entry.source.clone()) else {
            return self.save(brush);
        };
        let pack: Vec<Brush> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.source.as_ref() == Some(&path))
            .map(|(at, entry)| if at == index { brush.clone() } else { (*entry.brush).clone() })
            .collect();
        write_pack(&path, &pack)?;
        *self = Self::load();
        Ok(())
    }

    pub fn name_taken(&self, name: &str, except: Option<usize>) -> bool {
        self.entries
            .iter()
            .enumerate()
            .any(|(at, entry)| Some(at) != except && entry.brush.meta.id.is_empty() && entry.brush.meta.name == name)
    }

    pub fn remove(&mut self, index: usize) -> Result<(), String> {
        let Some(path) = self
            .entries
            .get(index)
            .and_then(|entry| entry.source.clone())
        else {
            return Ok(());
        };
        let kept: Vec<Brush> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(at, entry)| *at != index && entry.source.as_ref() == Some(&path))
            .map(|(_, entry)| (*entry.brush).clone())
            .collect();
        if kept.is_empty() {
            std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        } else {
            write_pack(&path, &kept)?;
        }
        *self = Self::load();
        Ok(())
    }

    fn override_builtin(&mut self, index: usize, brush: Brush, original: &Brush) -> Result<(), String> {
        let modified = encode_pack(std::slice::from_ref(&brush)).map_err(|error| error.to_string())?
            != encode_pack(std::slice::from_ref(original)).map_err(|error| error.to_string())?;
        let mut overrides: Vec<Brush> = self.overrides.iter().filter(|changed| changed.meta.id != brush.meta.id).cloned().collect();
        if modified {
            overrides.push(brush.clone());
        }
        write_overrides(&overrides)?;
        self.overrides = overrides;
        if let Some(entry) = self.entries.get_mut(index) {
            entry.brush = Arc::new(brush);
            entry.modified = modified;
        }
        Ok(())
    }

    fn store(&mut self, brushes: &[Brush], stem: &str) -> Result<(), String> {
        let dir = crate::config::brushes_dir()
            .ok_or_else(|| crate::i18n::t(crate::i18n::Text::BrushNoLibraryDir).to_owned())?;
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        write_pack(&unique_path(&dir, stem), brushes)?;
        *self = Self::load();
        Ok(())
    }
}

fn default_brushes() -> Vec<Brush> {
    decode_pack(DEFAULT_PACK).unwrap_or_default()
}

fn builtin_original(id: &str) -> Option<Brush> {
    default_brushes().into_iter().find(|brush| !id.is_empty() && brush.meta.id == id)
}

fn load_overrides() -> Vec<Brush> {
    crate::config::brush_overrides_path().and_then(|path| read_pack(&path).ok()).unwrap_or_default()
}

fn write_overrides(overrides: &[Brush]) -> Result<(), String> {
    let path = crate::config::brush_overrides_path().ok_or_else(|| crate::i18n::t(crate::i18n::Text::BrushNoLibraryDir).to_owned())?;
    if overrides.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    write_pack(&path, overrides)
}

const DEFAULTS_REVISION: u32 = 5;
const REVISION_PREFIX: &str = "revision:";

fn seed_defaults() {
    let Some(dir) = crate::config::brushes_dir() else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let mut seeded = crate::config::load_seeded_brushes();
    let revision_marker = format!("{REVISION_PREFIX}{DEFAULTS_REVISION}");
    let outdated = !seeded.contains(&revision_marker);
    for brush in default_brushes() {
        let path = dir.join(format!("{}.{FILE_EXTENSION}", brush.meta.id));
        let known = seeded.contains(&brush.meta.id);
        if known && !(outdated && path.exists()) {
            continue;
        }
        let keeps_existing_file = !known && path.exists();
        if keeps_existing_file || write_pack(&path, std::slice::from_ref(&brush)).is_ok() {
            if !known {
                seeded.push(brush.meta.id);
            }
        }
    }
    seeded.retain(|line| !line.starts_with(REVISION_PREFIX));
    seeded.push(revision_marker);
    crate::config::save_seeded_brushes(&seeded);
}

fn user_files() -> Vec<PathBuf> {
    let Some(dir) = crate::config::brushes_dir() else {
        return Vec::new();
    };
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == FILE_EXTENSION)
        })
        .collect();
    files.sort();
    files
}

pub fn save_active(brush: &Brush, eraser: &Brush) -> Result<(), String> {
    let path = crate::config::active_brushes_path().ok_or_else(|| crate::i18n::t(crate::i18n::Text::BrushNoLibraryDir).to_owned())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    write_pack(&path, &[brush.clone(), eraser.clone()])
}

pub fn restore_active(mut options: ToolOptions) -> ToolOptions {
    let restored = crate::config::active_brushes_path().and_then(|path| read_pack(&path).ok()).and_then(|brushes| <[Brush; 2]>::try_from(brushes).ok());
    if let Some([brush, eraser]) = restored {
        options.brush = Arc::new(brush);
        options.eraser_brush = Arc::new(eraser);
    }
    options
}

fn read_pack(path: &Path) -> Result<Vec<Brush>, Box<dyn std::error::Error>> {
    Ok(decode_pack(&std::fs::read(path)?)?)
}

pub fn write_pack(path: &Path, brushes: &[Brush]) -> Result<(), String> {
    let bytes = encode_pack(brushes).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

fn file_stem(path: &Path) -> String {
    let raw = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        FALLBACK_FILE_STEM.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn unique_path(dir: &Path, stem: &str) -> PathBuf {
    let candidate = |suffix: Option<u32>| {
        dir.join(format!(
            "{stem}{}.{FILE_EXTENSION}",
            suffix.map(|n| format!("-{n}")).unwrap_or_default()
        ))
    };
    (1u32..)
        .map(|n| (n > 1).then_some(n))
        .map(candidate)
        .find(|path| !path.exists())
        .unwrap_or_else(|| candidate(None))
}

use std::borrow::Cow;

use chequer_core::raster::Raster;

#[cfg(target_os = "android")]
mod arboard {
    use std::borrow::Cow;

    #[derive(Debug)]
    pub enum Error {
        ContentNotAvailable,
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("системный буфер обмена недоступен")
        }
    }
    pub struct ImageData<'a> {
        pub width: usize,
        pub height: usize,
        pub bytes: Cow<'a, [u8]>,
    }
    pub struct Clipboard;
    impl Clipboard {
        pub fn new() -> Result<Self, Error> {
            Err(Error::ContentNotAvailable)
        }
        pub fn set_image(&mut self, _: ImageData<'_>) -> Result<(), Error> {
            Err(Error::ContentNotAvailable)
        }
        pub fn get_image(&mut self) -> Result<ImageData<'static>, Error> {
            Err(Error::ContentNotAvailable)
        }
    }
}

pub struct Clipboard {
    system: Option<arboard::Clipboard>,
    internal: Option<Raster>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self { system: arboard::Clipboard::new().ok(), internal: None }
    }

    pub fn set(&mut self, raster: &Raster) -> Result<(), String> {
        let Some(system) = self.system.as_mut() else {
            self.internal = Some(raster.clone());
            return Err("системный буфер обмена недоступен".to_owned());
        };
        let data = arboard::ImageData {
            width: raster.width() as usize,
            height: raster.height() as usize,
            bytes: Cow::Borrowed(raster.as_bytes()),
        };
        let result = system.set_image(data).map_err(|e| e.to_string());
        self.internal = result.is_err().then(|| raster.clone());
        result
    }

    pub fn get(&mut self) -> Result<Option<Raster>, String> {
        let from_system = match self.system.as_mut().map(|c| c.get_image()) {
            Some(Ok(image)) => Raster::from_rgba(image.width as u32, image.height as u32, image.bytes.into_owned()).ok(),
            Some(Err(arboard::Error::ContentNotAvailable)) | None => None,
            Some(Err(e)) => return self.internal.clone().map(Some).ok_or_else(|| e.to_string()),
        };
        Ok(from_system.or_else(|| self.internal.clone()))
    }
}

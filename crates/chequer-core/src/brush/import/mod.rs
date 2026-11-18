mod abr;
mod gbr;
mod kpp;

use super::{Brush, BrushError};

pub use gbr::decode_gbr;

pub const IMPORT_EXTENSIONS: [&str; 3] = ["gbr", "abr", "kpp"];

const TRUNCATED: BrushError = BrushError::Malformed("файл обрезан");

pub fn decode_foreign(extension: &str, stem: &str, bytes: &[u8]) -> Result<Vec<Brush>, BrushError> {
    match extension {
        "gbr" => decode_gbr(bytes).map(|brush| vec![brush]),
        "abr" => abr::decode_abr(bytes, stem),
        "kpp" => kpp::decode_kpp(bytes).map(|brush| vec![brush]),
        other => Err(BrushError::Unsupported(format!("формат .{other}"))),
    }
}

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    pub fn seek(&mut self, at: usize) -> Result<(), BrushError> {
        if at > self.bytes.len() {
            return Err(TRUNCATED);
        }
        self.at = at;
        Ok(())
    }

    pub fn take(&mut self, count: usize) -> Result<&'a [u8], BrushError> {
        let end = self.at.checked_add(count).filter(|&end| end <= self.bytes.len()).ok_or(TRUNCATED)?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    pub fn skip(&mut self, count: usize) -> Result<(), BrushError> {
        self.take(count).map(|_| ())
    }

    pub fn u8(&mut self) -> Result<u8, BrushError> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, BrushError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().map_err(|_| TRUNCATED)?))
    }

    pub fn u32(&mut self) -> Result<u32, BrushError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().map_err(|_| TRUNCATED)?))
    }

    pub fn i32(&mut self) -> Result<i32, BrushError> {
        Ok(self.u32()? as i32)
    }

    pub fn ucs2_text(&mut self) -> Result<String, BrushError> {
        let units = self.u32()? as usize;
        let raw = self.take(units.checked_mul(2).ok_or(TRUNCATED)?)?;
        let decoded: Vec<u16> = raw.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect();
        Ok(String::from_utf16_lossy(&decoded).trim_end_matches('\0').to_owned())
    }
}

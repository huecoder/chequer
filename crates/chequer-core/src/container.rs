use std::fmt;

const MAGIC_SIZE: usize = 4;
const VERSION_SIZE: usize = 2;
const FILE_HEADER_SIZE: usize = MAGIC_SIZE + 2 * VERSION_SIZE;
const TAG_SIZE: usize = 4;
const CHUNK_HEADER_SIZE: usize = TAG_SIZE + 1 + 4;
const CHECKSUM_SIZE: usize = 4;
const CRITICAL_FLAG: u8 = 1;
pub const MAX_CHUNK_SIZE: usize = 256 * 1024 * 1024;

pub type Tag = [u8; TAG_SIZE];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub tag: Tag,
    pub critical: bool,
    pub payload: Vec<u8>,
}

impl Chunk {
    pub fn new(tag: Tag, payload: Vec<u8>) -> Self {
        Self { tag, critical: false, payload }
    }

    pub fn critical(tag: Tag, payload: Vec<u8>) -> Self {
        Self { tag, critical: true, payload }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub major: u16,
    pub minor: u16,
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ContainerError {
    BadMagic,
    Truncated,
    ChunkTooLarge { tag: Tag },
    Corrupt { tag: Tag },
    UnsupportedMajor { found: u16, supported: u16 },
    UnknownCritical { tag: Tag },
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => write!(f, "это не файл Chequer нужного типа"),
            Self::Truncated => write!(f, "файл обрезан"),
            Self::ChunkTooLarge { tag } => write!(f, "блок «{}» слишком велик", tag_name(tag)),
            Self::Corrupt { tag } => write!(f, "блок «{}» повреждён", tag_name(tag)),
            Self::UnsupportedMajor { found, supported } => write!(f, "версия формата {found} новее поддерживаемой {supported}"),
            Self::UnknownCritical { tag } => write!(f, "неизвестный обязательный блок «{}»", tag_name(tag)),
        }
    }
}

impl std::error::Error for ContainerError {}

fn tag_name(tag: &Tag) -> String {
    tag.iter().map(|&byte| if byte.is_ascii_graphic() { byte as char } else { '?' }).collect()
}

fn checksum(header: &[u8], payload: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(header);
    hasher.update(payload);
    hasher.finalize()
}

impl Container {
    pub fn new(major: u16, minor: u16) -> Self {
        Self { major, minor, chunks: Vec::new() }
    }

    pub fn encode(&self, magic: Tag) -> Vec<u8> {
        let body: usize = self.chunks.iter().map(|chunk| CHUNK_HEADER_SIZE + chunk.payload.len() + CHECKSUM_SIZE).sum();
        let mut bytes = Vec::with_capacity(FILE_HEADER_SIZE + body);
        bytes.extend_from_slice(&magic);
        bytes.extend_from_slice(&self.major.to_le_bytes());
        bytes.extend_from_slice(&self.minor.to_le_bytes());
        for chunk in &self.chunks {
            let mut header = [0u8; CHUNK_HEADER_SIZE];
            header[..TAG_SIZE].copy_from_slice(&chunk.tag);
            header[TAG_SIZE] = if chunk.critical { CRITICAL_FLAG } else { 0 };
            header[TAG_SIZE + 1..].copy_from_slice(&(chunk.payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&header);
            bytes.extend_from_slice(&chunk.payload);
            bytes.extend_from_slice(&checksum(&header, &chunk.payload).to_le_bytes());
        }
        bytes
    }

    pub fn decode(bytes: &[u8], magic: Tag, supported_major: u16) -> Result<Self, ContainerError> {
        let header = bytes.get(..FILE_HEADER_SIZE).ok_or(ContainerError::Truncated)?;
        if header[..MAGIC_SIZE] != magic {
            return Err(ContainerError::BadMagic);
        }
        let major = u16::from_le_bytes([header[MAGIC_SIZE], header[MAGIC_SIZE + 1]]);
        let minor = u16::from_le_bytes([header[MAGIC_SIZE + 2], header[MAGIC_SIZE + 3]]);
        if major != supported_major {
            return Err(ContainerError::UnsupportedMajor { found: major, supported: supported_major });
        }
        let mut container = Self::new(major, minor);
        let mut rest = &bytes[FILE_HEADER_SIZE..];
        while !rest.is_empty() {
            let (chunk, tail) = read_chunk(rest)?;
            container.chunks.push(chunk);
            rest = tail;
        }
        Ok(container)
    }

    pub fn reject_unknown_critical(&self, known: &[Tag]) -> Result<(), ContainerError> {
        match self.chunks.iter().find(|chunk| chunk.critical && !known.contains(&chunk.tag)) {
            Some(chunk) => Err(ContainerError::UnknownCritical { tag: chunk.tag }),
            None => Ok(()),
        }
    }
}

fn read_chunk(bytes: &[u8]) -> Result<(Chunk, &[u8]), ContainerError> {
    let header = bytes.get(..CHUNK_HEADER_SIZE).ok_or(ContainerError::Truncated)?;
    let tag: Tag = header[..TAG_SIZE].try_into().map_err(|_| ContainerError::Truncated)?;
    let length = u32::from_le_bytes([header[TAG_SIZE + 1], header[TAG_SIZE + 2], header[TAG_SIZE + 3], header[TAG_SIZE + 4]]) as usize;
    if length > MAX_CHUNK_SIZE {
        return Err(ContainerError::ChunkTooLarge { tag });
    }
    let payload_end = CHUNK_HEADER_SIZE + length;
    let payload = bytes.get(CHUNK_HEADER_SIZE..payload_end).ok_or(ContainerError::Truncated)?;
    let stored = bytes.get(payload_end..payload_end + CHECKSUM_SIZE).ok_or(ContainerError::Truncated)?;
    if u32::from_le_bytes([stored[0], stored[1], stored[2], stored[3]]) != checksum(header, payload) {
        return Err(ContainerError::Corrupt { tag });
    }
    let chunk = Chunk { tag, critical: header[TAG_SIZE] & CRITICAL_FLAG != 0, payload: payload.to_vec() };
    Ok((chunk, &bytes[payload_end + CHECKSUM_SIZE..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAGIC: Tag = *b"TST\0";

    fn sample() -> Container {
        let mut container = Container::new(1, 3);
        container.chunks.push(Chunk::critical(*b"HEAD", vec![1, 2, 3]));
        container.chunks.push(Chunk::new(*b"BODY", Vec::new()));
        container
    }

    #[test]
    fn encode_then_decode_returns_the_same_chunks() {
        let container = sample();
        assert_eq!(Container::decode(&container.encode(MAGIC), MAGIC, 1), Ok(container));
    }

    #[test]
    fn flipped_byte_is_reported_as_corrupt_chunk() {
        let mut bytes = sample().encode(MAGIC);
        bytes[FILE_HEADER_SIZE + CHUNK_HEADER_SIZE] ^= 0xFF;
        assert_eq!(Container::decode(&bytes, MAGIC, 1), Err(ContainerError::Corrupt { tag: *b"HEAD" }));
    }

    #[test]
    fn truncated_file_and_foreign_magic_and_newer_major_are_rejected() {
        let bytes = sample().encode(MAGIC);
        assert_eq!(Container::decode(&bytes[..bytes.len() - 1], MAGIC, 1), Err(ContainerError::Truncated));
        assert_eq!(Container::decode(&bytes, *b"OTH\0", 1), Err(ContainerError::BadMagic));
        assert_eq!(Container::decode(&bytes, MAGIC, 0), Err(ContainerError::UnsupportedMajor { found: 1, supported: 0 }));
    }

    #[test]
    fn unknown_critical_chunk_is_refused_but_unknown_optional_is_not() {
        let container = sample();
        assert_eq!(container.reject_unknown_critical(&[*b"HEAD"]), Ok(()));
        assert_eq!(container.reject_unknown_critical(&[]), Err(ContainerError::UnknownCritical { tag: *b"HEAD" }));
    }

    #[test]
    fn declared_length_beyond_the_file_does_not_allocate() {
        let mut bytes = sample().encode(MAGIC);
        bytes[FILE_HEADER_SIZE + TAG_SIZE + 1..FILE_HEADER_SIZE + CHUNK_HEADER_SIZE].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Container::decode(&bytes, MAGIC, 1), Err(ContainerError::ChunkTooLarge { tag: *b"HEAD" }));
    }
}

use crate::container::ContainerError;

use super::ChqError;

const VARINT_PAYLOAD_BITS: u32 = 7;
const VARINT_PAYLOAD_MASK: u8 = 0x7F;
const VARINT_CONTINUE: u8 = 0x80;
const VARINT_MAX_SHIFT: u32 = 63;

pub struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }

    pub fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }

    pub fn take(&mut self, count: usize) -> Result<&'a [u8], ChqError> {
        if count > self.rest.len() {
            return Err(ContainerError::Truncated.into());
        }
        let (head, tail) = self.rest.split_at(count);
        self.rest = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ChqError> {
        let mut array = [0u8; N];
        array.copy_from_slice(self.take(N)?);
        Ok(array)
    }

    pub fn u8(&mut self) -> Result<u8, ChqError> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16, ChqError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, ChqError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64, ChqError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub fn u128(&mut self) -> Result<u128, ChqError> {
        Ok(u128::from_le_bytes(self.array()?))
    }

    pub fn tag(&mut self) -> Result<[u8; 4], ChqError> {
        self.array()
    }

    pub fn varint(&mut self) -> Result<u64, ChqError> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = self.u8()?;
            value |= u64::from(byte & VARINT_PAYLOAD_MASK) << shift;
            if byte & VARINT_CONTINUE == 0 {
                return Ok(value);
            }
            shift += VARINT_PAYLOAD_BITS;
            if shift > VARINT_MAX_SHIFT {
                return Err(ChqError::Invalid("слишком длинное число"));
            }
        }
    }
}

pub fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= u64::from(VARINT_CONTINUE) {
        out.push((value as u8 & VARINT_PAYLOAD_MASK) | VARINT_CONTINUE);
        value >>= VARINT_PAYLOAD_BITS;
    }
    out.push(value as u8);
}

pub fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

pub fn unzigzag(value: u64) -> i64 {
    (value >> 1) as i64 ^ -((value & 1) as i64)
}

use std::collections::HashMap;
use std::io::Read;

use flate2::read::GzDecoder;

use crate::blend::BlendMode;
use crate::document::{Document, Layer, MAX_SIDE};
use crate::io::IoError;
use crate::ora::invalid;
use crate::raster::Raster;

pub const FILE_EXTENSION: &str = "pdn";

const MAGIC: &[u8] = b"PDN3";
const HEADER_LENGTH_BYTES: usize = 3;
const DATA_MARKER: [u8; 2] = [0x00, 0x01];
const CHUNKS_GZIP: u8 = 0;
const CHUNKS_RAW: u8 = 1;
const BYTES_PER_PIXEL: usize = 4;
const VARINT_MAX_BYTES: usize = 5;
const VARINT_PAYLOAD_BITS: u32 = 7;
const VARINT_PAYLOAD_MASK: u8 = 0x7f;
const VARINT_CONTINUE: u8 = 0x80;
const MAX_NESTING: usize = 64;
const MAX_ARRAY_LENGTH: usize = 1 << 20;
const MAX_OBJECTS: usize = 1 << 22;
const BLEND_OP_SUFFIX: &str = "BlendOp";

const BLEND_OP_NAMES: [(&str, BlendMode); 14] = [
    ("Normal", BlendMode::Normal),
    ("Multiply", BlendMode::Multiply),
    ("Additive", BlendMode::Additive),
    ("ColorBurn", BlendMode::ColorBurn),
    ("ColorDodge", BlendMode::ColorDodge),
    ("Reflect", BlendMode::Reflect),
    ("Glow", BlendMode::Glow),
    ("Overlay", BlendMode::Overlay),
    ("Difference", BlendMode::Difference),
    ("Negation", BlendMode::Negation),
    ("Lighten", BlendMode::Lighten),
    ("Darken", BlendMode::Darken),
    ("Screen", BlendMode::Screen),
    ("Xor", BlendMode::Xor),
];

mod record {
    pub const HEADER: u8 = 0;
    pub const CLASS_WITH_ID: u8 = 1;
    pub const SYSTEM_CLASS: u8 = 2;
    pub const CLASS: u8 = 3;
    pub const SYSTEM_CLASS_TYPED: u8 = 4;
    pub const CLASS_TYPED: u8 = 5;
    pub const STRING: u8 = 6;
    pub const BINARY_ARRAY: u8 = 7;
    pub const PRIMITIVE_TYPED: u8 = 8;
    pub const REFERENCE: u8 = 9;
    pub const NULL: u8 = 10;
    pub const END: u8 = 11;
    pub const LIBRARY: u8 = 12;
    pub const NULL_MULTIPLE_256: u8 = 13;
    pub const NULL_MULTIPLE: u8 = 14;
    pub const PRIMITIVE_ARRAY: u8 = 15;
    pub const OBJECT_ARRAY: u8 = 16;
    pub const STRING_ARRAY: u8 = 17;
}

mod binary_type {
    pub const PRIMITIVE: u8 = 0;
    pub const SYSTEM_CLASS: u8 = 3;
    pub const CLASS: u8 = 4;
    pub const PRIMITIVE_ARRAY: u8 = 7;
}

mod array_kind {
    pub const SINGLE_OFFSET: u8 = 3;
    pub const JAGGED_OFFSET: u8 = 4;
    pub const RECTANGULAR_OFFSET: u8 = 5;
}

mod primitive {
    pub const BOOLEAN: u8 = 1;
    pub const BYTE: u8 = 2;
    pub const CHAR: u8 = 3;
    pub const DECIMAL: u8 = 5;
    pub const DOUBLE: u8 = 6;
    pub const INT16: u8 = 7;
    pub const INT32: u8 = 8;
    pub const INT64: u8 = 9;
    pub const SBYTE: u8 = 10;
    pub const SINGLE: u8 = 11;
    pub const TIME_SPAN: u8 = 12;
    pub const DATE_TIME: u8 = 13;
    pub const UINT16: u8 = 14;
    pub const UINT32: u8 = 15;
    pub const UINT64: u8 = 16;
    pub const STRING: u8 = 18;
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], IoError> {
        if count > self.remaining() {
            return Err(invalid("pdn: unexpected end of data"));
        }
        let slice = &self.bytes[self.position..self.position + count];
        self.position += count;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], IoError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, IoError> {
        Ok(self.array::<1>()?[0])
    }

    fn i32(&mut self) -> Result<i32, IoError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    fn u32_big_endian(&mut self) -> Result<u32, IoError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn length(&mut self) -> Result<usize, IoError> {
        usize::try_from(self.i32()?).map_err(|_| invalid("pdn: negative length"))
    }

    fn varint(&mut self) -> Result<usize, IoError> {
        let mut value = 0usize;
        for index in 0..VARINT_MAX_BYTES {
            let byte = self.u8()?;
            value |= usize::from(byte & VARINT_PAYLOAD_MASK) << (VARINT_PAYLOAD_BITS * index as u32);
            if byte & VARINT_CONTINUE == 0 {
                return Ok(value);
            }
        }
        Err(invalid("pdn: bad string length"))
    }

    fn text(&mut self) -> Result<String, IoError> {
        let length = self.varint()?;
        String::from_utf8(self.take(length)?.to_vec()).map_err(invalid)
    }
}

#[derive(Clone, Debug)]
enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    Ref(i32),
    Object(Object),
    List(Vec<Value>),
}

#[derive(Clone, Debug)]
struct Object {
    class: String,
    members: Vec<(String, Value)>,
}

impl Object {
    fn member(&self, name: &str) -> Option<&Value> {
        self.members.iter().find(|(member, _)| member == name).map(|(_, value)| value)
    }

    fn inherited_member(&self, name: &str) -> Option<&Value> {
        self.member(name).or_else(|| self.members.iter().find(|(member, _)| member.rsplit('+').next() == Some(name)).map(|(_, value)| value))
    }
}

#[derive(Clone, Debug)]
struct Class {
    name: String,
    members: Vec<String>,
    kinds: Vec<Option<u8>>,
}

enum Item {
    Single(Value),
    Nulls(usize),
    End,
}

struct Graph {
    root: i32,
    objects: HashMap<i32, Value>,
}

impl Graph {
    fn resolve<'a>(&'a self, value: &'a Value) -> &'a Value {
        match value {
            Value::Ref(id) => self.objects.get(id).unwrap_or(&Value::Null),
            other => other,
        }
    }

    fn root(&self) -> Option<&Object> {
        match self.objects.get(&self.root) {
            Some(Value::Object(object)) => Some(object),
            _ => None,
        }
    }

    fn object<'a>(&'a self, value: Option<&'a Value>) -> Option<&'a Object> {
        match self.resolve(value?) {
            Value::Object(object) => Some(object),
            _ => None,
        }
    }

    fn int(&self, value: Option<&Value>) -> Option<i64> {
        match self.resolve(value?) {
            Value::Int(number) => Some(*number),
            _ => None,
        }
    }

    fn boolean(&self, value: Option<&Value>) -> Option<bool> {
        match self.resolve(value?) {
            Value::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    fn text<'a>(&'a self, value: Option<&'a Value>) -> Option<&'a str> {
        match self.resolve(value?) {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    fn list<'a>(&'a self, value: Option<&'a Value>) -> Option<&'a [Value]> {
        match self.resolve(value?) {
            Value::List(items) => Some(items),
            _ => None,
        }
    }
}

struct Parser<'a> {
    reader: Reader<'a>,
    classes: HashMap<i32, Class>,
    objects: HashMap<i32, Value>,
}

impl<'a> Parser<'a> {
    fn parse(mut self) -> Result<(Graph, usize), IoError> {
        if self.reader.u8()? != record::HEADER {
            return Err(invalid("pdn: stream header missing"));
        }
        let root = self.reader.i32()?;
        self.reader.take(3 * std::mem::size_of::<i32>())?;
        loop {
            match self.record(0)? {
                Item::End => break,
                Item::Single(_) | Item::Nulls(_) => {}
            }
        }
        Ok((Graph { root, objects: self.objects }, self.reader.position))
    }

    fn store(&mut self, id: i32, value: Value) -> Result<Value, IoError> {
        if self.objects.len() >= MAX_OBJECTS {
            return Err(invalid("pdn: too many objects"));
        }
        self.objects.insert(id, value);
        Ok(Value::Ref(id))
    }

    fn primitive(&mut self, kind: u8) -> Result<Value, IoError> {
        Ok(match kind {
            primitive::BOOLEAN => Value::Bool(self.reader.u8()? != 0),
            primitive::BYTE => Value::Int(i64::from(self.reader.u8()?)),
            primitive::SBYTE => Value::Int(i64::from(i8::from_le_bytes(self.reader.array()?))),
            primitive::CHAR => {
                let first = self.reader.u8()?;
                let extra = match first {
                    0xc0..=0xdf => 1,
                    0xe0..=0xef => 2,
                    0xf0..=0xf7 => 3,
                    _ => 0,
                };
                self.reader.take(extra)?;
                Value::Null
            }
            primitive::INT16 => Value::Int(i64::from(i16::from_le_bytes(self.reader.array()?))),
            primitive::UINT16 => Value::Int(i64::from(u16::from_le_bytes(self.reader.array()?))),
            primitive::INT32 => Value::Int(i64::from(self.reader.i32()?)),
            primitive::UINT32 => Value::Int(i64::from(u32::from_le_bytes(self.reader.array()?))),
            primitive::INT64 | primitive::TIME_SPAN | primitive::DATE_TIME => Value::Int(i64::from_le_bytes(self.reader.array()?)),
            primitive::UINT64 => Value::Int(i64::try_from(u64::from_le_bytes(self.reader.array()?)).unwrap_or(i64::MAX)),
            primitive::SINGLE | primitive::DOUBLE => {
                self.reader.take(Self::primitive_size(kind)?)?;
                Value::Null
            }
            primitive::DECIMAL | primitive::STRING => Value::Text(self.reader.text()?),
            _ => return Err(invalid("pdn: unknown primitive type")),
        })
    }

    fn primitive_size(kind: u8) -> Result<usize, IoError> {
        Ok(match kind {
            primitive::BOOLEAN | primitive::BYTE | primitive::SBYTE => 1,
            primitive::INT16 | primitive::UINT16 => 2,
            primitive::INT32 | primitive::UINT32 | primitive::SINGLE => 4,
            primitive::INT64 | primitive::UINT64 | primitive::DOUBLE | primitive::TIME_SPAN | primitive::DATE_TIME => 8,
            _ => return Err(invalid("pdn: unsupported primitive array")),
        })
    }

    fn class_info(&mut self) -> Result<(i32, String, Vec<String>), IoError> {
        let id = self.reader.i32()?;
        let name = self.reader.text()?;
        let count = self.reader.length()?;
        if count > self.reader.remaining() {
            return Err(invalid("pdn: bad member count"));
        }
        let members = (0..count).map(|_| self.reader.text()).collect::<Result<_, _>>()?;
        Ok((id, name, members))
    }

    fn member_kinds(&mut self, count: usize) -> Result<Vec<Option<u8>>, IoError> {
        let types = (0..count).map(|_| self.reader.u8()).collect::<Result<Vec<_>, _>>()?;
        let mut kinds = Vec::with_capacity(count);
        for binary in types {
            kinds.push(match binary {
                binary_type::PRIMITIVE => Some(self.reader.u8()?),
                binary_type::PRIMITIVE_ARRAY => {
                    self.reader.u8()?;
                    None
                }
                binary_type::SYSTEM_CLASS => {
                    self.reader.text()?;
                    None
                }
                binary_type::CLASS => {
                    self.reader.text()?;
                    self.reader.i32()?;
                    None
                }
                _ => None,
            });
        }
        Ok(kinds)
    }

    fn members(&mut self, class: &Class, depth: usize) -> Result<Vec<(String, Value)>, IoError> {
        let mut members = Vec::with_capacity(class.members.len());
        for (name, kind) in class.members.iter().zip(&class.kinds) {
            let value = match kind {
                Some(kind) => self.primitive(*kind)?,
                None => match self.record(depth + 1)? {
                    Item::Single(value) => value,
                    Item::Nulls(1) => Value::Null,
                    Item::Nulls(_) | Item::End => return Err(invalid("pdn: bad member value")),
                },
            };
            members.push((name.clone(), value));
        }
        Ok(members)
    }

    fn object(&mut self, id: i32, class: &Class, depth: usize) -> Result<Value, IoError> {
        let members = self.members(class, depth)?;
        self.store(id, Value::Object(Object { class: class.name.clone(), members }))
    }

    fn elements(&mut self, length: usize, depth: usize) -> Result<Vec<Value>, IoError> {
        if length > MAX_ARRAY_LENGTH {
            return Err(invalid("pdn: array too long"));
        }
        let mut items = Vec::new();
        while items.len() < length {
            match self.record(depth + 1)? {
                Item::Single(value) => items.push(value),
                Item::Nulls(count) => items.extend(std::iter::repeat_n(Value::Null, count.min(length - items.len()))),
                Item::End => return Err(invalid("pdn: array cut short")),
            }
        }
        Ok(items)
    }

    fn binary_array(&mut self, id: i32, depth: usize) -> Result<Value, IoError> {
        let kind = self.reader.u8()?;
        let rank = self.reader.length()?;
        if rank == 0 || rank > self.reader.remaining() {
            return Err(invalid("pdn: bad array rank"));
        }
        let mut total = 1usize;
        for _ in 0..rank {
            total = total.checked_mul(self.reader.length()?).filter(|total| *total <= MAX_ARRAY_LENGTH).ok_or_else(|| invalid("pdn: array too long"))?;
        }
        if matches!(kind, array_kind::SINGLE_OFFSET | array_kind::JAGGED_OFFSET | array_kind::RECTANGULAR_OFFSET) {
            self.reader.take(rank * std::mem::size_of::<i32>())?;
        }
        let element = self.reader.u8()?;
        let primitive_kind = match element {
            binary_type::PRIMITIVE | binary_type::PRIMITIVE_ARRAY => Some(self.reader.u8()?),
            binary_type::SYSTEM_CLASS => {
                self.reader.text()?;
                None
            }
            binary_type::CLASS => {
                self.reader.text()?;
                self.reader.i32()?;
                None
            }
            _ => None,
        };
        let items = match (element, primitive_kind) {
            (binary_type::PRIMITIVE, Some(kind)) => {
                self.reader.take(Self::primitive_size(kind)?.checked_mul(total).ok_or_else(|| invalid("pdn: array too long"))?)?;
                Vec::new()
            }
            _ => self.elements(total, depth)?,
        };
        self.store(id, Value::List(items))
    }

    fn record(&mut self, depth: usize) -> Result<Item, IoError> {
        if depth > MAX_NESTING {
            return Err(invalid("pdn: nesting too deep"));
        }
        loop {
            let tag = self.reader.u8()?;
            let value = match tag {
                record::LIBRARY => {
                    self.reader.i32()?;
                    self.reader.text()?;
                    continue;
                }
                record::END => return Ok(Item::End),
                record::NULL => return Ok(Item::Nulls(1)),
                record::NULL_MULTIPLE_256 => return Ok(Item::Nulls(usize::from(self.reader.u8()?))),
                record::NULL_MULTIPLE => return Ok(Item::Nulls(self.reader.length()?)),
                record::REFERENCE => Value::Ref(self.reader.i32()?),
                record::PRIMITIVE_TYPED => {
                    let kind = self.reader.u8()?;
                    self.primitive(kind)?
                }
                record::STRING => {
                    let id = self.reader.i32()?;
                    let text = self.reader.text()?;
                    self.store(id, Value::Text(text))?
                }
                record::CLASS_WITH_ID => {
                    let id = self.reader.i32()?;
                    let metadata = self.reader.i32()?;
                    let class = self.classes.get(&metadata).cloned().ok_or_else(|| invalid("pdn: unknown class reference"))?;
                    self.object(id, &class, depth)?
                }
                record::SYSTEM_CLASS | record::CLASS => {
                    let (id, name, members) = self.class_info()?;
                    if tag == record::CLASS {
                        self.reader.i32()?;
                    }
                    let kinds = vec![None; members.len()];
                    let class = Class { name, members, kinds };
                    self.classes.insert(id, class.clone());
                    self.object(id, &class, depth)?
                }
                record::SYSTEM_CLASS_TYPED | record::CLASS_TYPED => {
                    let (id, name, members) = self.class_info()?;
                    let kinds = self.member_kinds(members.len())?;
                    if tag == record::CLASS_TYPED {
                        self.reader.i32()?;
                    }
                    let class = Class { name, members, kinds };
                    self.classes.insert(id, class.clone());
                    self.object(id, &class, depth)?
                }
                record::BINARY_ARRAY => {
                    let id = self.reader.i32()?;
                    self.binary_array(id, depth)?
                }
                record::PRIMITIVE_ARRAY => {
                    let id = self.reader.i32()?;
                    let length = self.reader.length()?;
                    let kind = self.reader.u8()?;
                    self.reader.take(Self::primitive_size(kind)?.checked_mul(length).ok_or_else(|| invalid("pdn: array too long"))?)?;
                    self.store(id, Value::List(Vec::new()))?
                }
                record::OBJECT_ARRAY | record::STRING_ARRAY => {
                    let id = self.reader.i32()?;
                    let length = self.reader.length()?;
                    let items = self.elements(length, depth)?;
                    self.store(id, Value::List(items))?
                }
                _ => return Err(invalid("pdn: unknown record")),
            };
            return Ok(Item::Single(value));
        }
    }
}

fn parse_graph(bytes: &[u8]) -> Result<(Graph, usize), IoError> {
    Parser { reader: Reader::new(bytes), classes: HashMap::new(), objects: HashMap::new() }.parse()
}

fn read_pixels(reader: &mut Reader, length: usize) -> Result<Vec<u8>, IoError> {
    let format = reader.u8()?;
    let chunk_size = reader.u32_big_endian()? as usize;
    if chunk_size == 0 || !matches!(format, CHUNKS_GZIP | CHUNKS_RAW) {
        return Err(invalid("pdn: bad pixel block"));
    }
    let chunk_count = length.div_ceil(chunk_size);
    let mut pixels = vec![0u8; length];
    let mut seen = vec![false; chunk_count];
    for _ in 0..chunk_count {
        let number = reader.u32_big_endian()? as usize;
        let size = reader.u32_big_endian()? as usize;
        let packed = reader.take(size)?;
        if number >= chunk_count || std::mem::replace(&mut seen[number], true) {
            return Err(invalid("pdn: bad chunk number"));
        }
        let offset = number * chunk_size;
        let target = &mut pixels[offset..length.min(offset + chunk_size)];
        if format == CHUNKS_GZIP {
            GzDecoder::new(packed).read_exact(target).map_err(invalid)?;
        } else if packed.len() == target.len() {
            target.copy_from_slice(packed);
        } else {
            return Err(invalid("pdn: bad chunk size"));
        }
    }
    Ok(pixels)
}

fn blend_from_value(graph: &Graph, properties: &Object, layer: &Object) -> BlendMode {
    if let Some(index) = graph.object(properties.member("blendMode")).and_then(|mode| graph.int(mode.member("value__"))) {
        return usize::try_from(index).ok().and_then(|index| BlendMode::ALL.get(index).copied()).unwrap_or_default();
    }
    let operation = graph.object(layer.member("properties")).and_then(|own| graph.object(own.member("blendOp")));
    operation
        .and_then(|operation| operation.class.rsplit('.').next())
        .and_then(|name| name.strip_suffix(BLEND_OP_SUFFIX))
        .and_then(|name| BLEND_OP_NAMES.iter().find(|(known, _)| name.ends_with(known)))
        .map_or_else(BlendMode::default, |(_, mode)| *mode)
}

fn dimension(graph: &Graph, object: &Object, name: &str) -> Result<u32, IoError> {
    graph
        .int(object.inherited_member(name))
        .and_then(|side| u32::try_from(side).ok())
        .filter(|side| (1..=MAX_SIDE).contains(side))
        .ok_or_else(|| invalid(format!("pdn: {name}")))
}

fn rgba_from_bgra(mut pixels: Vec<u8>) -> Vec<u8> {
    for pixel in pixels.chunks_exact_mut(BYTES_PER_PIXEL) {
        pixel.swap(0, 2);
    }
    pixels
}

pub fn decode_document(bytes: &[u8]) -> Result<Document, IoError> {
    let mut reader = Reader::new(bytes);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(invalid("pdn: not a Paint.NET document"));
    }
    let header_length = reader.take(HEADER_LENGTH_BYTES)?.iter().rev().fold(0usize, |length, byte| (length << 8) | usize::from(*byte));
    reader.take(header_length)?;
    if reader.take(DATA_MARKER.len())? != DATA_MARKER {
        return Err(invalid("pdn: unsupported data layout"));
    }
    let (graph, consumed) = parse_graph(&bytes[reader.position..])?;
    reader.take(consumed)?;
    let document = graph.root().ok_or_else(|| invalid("pdn: document missing"))?;
    let (width, height) = (dimension(&graph, document, "width")?, dimension(&graph, document, "height")?);
    let list = graph.object(document.member("layers")).ok_or_else(|| invalid("pdn: layers missing"))?;
    let items = graph.list(list.inherited_member("_items")).ok_or_else(|| invalid("pdn: layers missing"))?;
    let count = graph.int(list.inherited_member("_size")).and_then(|count| usize::try_from(count).ok()).unwrap_or(items.len()).min(items.len());
    let mut layers = Vec::with_capacity(count);
    for item in &items[..count] {
        let layer = graph.object(Some(item)).ok_or_else(|| invalid("pdn: layer"))?;
        let properties = graph.object(layer.member("Layer+properties").or_else(|| layer.member("properties"))).ok_or_else(|| invalid("pdn: layer properties"))?;
        let surface = graph.object(layer.member("surface")).ok_or_else(|| invalid("pdn: layer surface"))?;
        let (surface_width, surface_height) = (dimension(&graph, surface, "width")?, dimension(&graph, surface, "height")?);
        let stride = graph.int(surface.member("stride")).and_then(|stride| usize::try_from(stride).ok());
        if (surface_width, surface_height) != (width, height) || stride != Some(width as usize * BYTES_PER_PIXEL) {
            return Err(invalid("pdn: unsupported layer format"));
        }
        let pixels = rgba_from_bgra(read_pixels(&mut reader, width as usize * height as usize * BYTES_PER_PIXEL)?);
        let raster = Raster::from_rgba(width, height, pixels).map_err(IoError::Raster)?;
        let mut result = Layer::new(graph.text(properties.member("name")).unwrap_or_default().to_owned(), raster);
        result.visible = graph.boolean(properties.member("visible")).unwrap_or(true);
        result.opacity = graph.int(properties.member("opacity")).map_or(u8::MAX, |opacity| opacity.clamp(0, i64::from(u8::MAX)) as u8);
        result.blend = blend_from_value(&graph, properties, layer);
        layers.push(result);
    }
    if layers.is_empty() {
        return Err(invalid("pdn: no layers"));
    }
    let top = layers.len() - 1;
    Document::from_layers(layers, top).map_err(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_LAYERS: &[u8] = include_bytes!("../tests/data/two_layers_additive.pdn");
    const TRUNCATION_STEP: usize = 97;

    #[test]
    fn layers_and_properties_are_read() {
        let document = decode_document(TWO_LAYERS).unwrap();
        assert_eq!((document.width(), document.height()), (800, 600));
        assert_eq!(document.layers().len(), 2);
        assert_eq!(document.layers()[0].name, "Background");
        assert_eq!(document.layers()[0].blend, BlendMode::Normal);
        assert_eq!(document.layers()[1].name, "Layer 2");
        assert_eq!(document.layers()[1].opacity, 161);
        assert_eq!(document.layers()[1].blend, BlendMode::Additive);
        assert!(document.layers()[1].visible);
    }

    #[test]
    fn truncated_files_fail_without_panic() {
        for length in (0..TWO_LAYERS.len()).step_by(TRUNCATION_STEP) {
            assert!(decode_document(&TWO_LAYERS[..length]).is_err());
        }
    }

    #[test]
    fn foreign_data_is_rejected() {
        assert!(decode_document(b"not a document").is_err());
    }
}

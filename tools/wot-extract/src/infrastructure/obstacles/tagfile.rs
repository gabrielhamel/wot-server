//! Havok tagfiles (`TAG0`, Havok 2016 and later), as WoT ships its collision models.
//!
//! A tagfile is a tree of sections, each a big-endian `u32` holding its size (section
//! header included) in its low 30 bits, then a 4-byte tag. `TAG0` holds `SDKV` (the
//! SDK version), `DATA` (the objects' bytes), `TYPE` (the type descriptions) and
//! `INDX` (the items: where each object or array is in `DATA`).
//!
//! Objects are laid out as their types describe, except that every pointer, string and
//! variable-length array is a `u32` index in the items, the item giving its type, its
//! place in `DATA` and its element count.

use thiserror::Error;

/// A tagfile's objects and their types, read whole.
#[derive(Debug)]
pub struct Tagfile {
    data: Vec<u8>,
    types: Vec<Type>,
    items: Vec<Item>,
}

/// A type description; its index in the types is its id, 0 being none.
#[derive(Debug, Default)]
struct Type {
    name: String,
    parent: usize,
    /// Its own format, `None` if it inherits its parent's.
    format: Option<u32>,
    /// What a pointer or array type points to.
    pointee: usize,
    /// Its own size, `None` if it inherits its parent's.
    size: Option<usize>,
    /// Its own members, not its parent's.
    members: Vec<Member>,
}

#[derive(Debug)]
struct Member {
    name: String,
    offset: usize,
    ty: usize,
}

/// An object or an array in `DATA`.
#[derive(Debug)]
struct Item {
    ty: usize,
    /// Whether it is a single object rather than an array.
    single: bool,
    offset: usize,
    count: usize,
}

/// The kinds of type formats, in a format's low 4 bits.
mod format {
    pub const STRING: u32 = 3;
    pub const INT: u32 = 4;
    pub const FLOAT: u32 = 5;
    pub const POINTER: u32 = 6;
    pub const RECORD: u32 = 7;
    pub const ARRAY: u32 = 8;
    /// An array of a fixed length, given in the bits above the low 8, held inline.
    pub const FIXED_ARRAY: u32 = 0x20;
    pub const SIGNED: u32 = 0x200;
}

impl Tagfile {
    /// Read a whole tagfile.
    pub fn from_bytes(file: &[u8]) -> Result<Self, DeError> {
        let (tag, root) = sections(file).next().ok_or(DeError::InvalidMagic)??;
        if tag != *b"TAG0" {
            return Err(DeError::InvalidMagic);
        }

        let mut data = None;
        let mut type_sections = None;
        let mut index = None;
        for section in sections(root) {
            match section? {
                (tag, content) if tag == *b"DATA" => data = Some(content),
                (tag, content) if tag == *b"TYPE" => type_sections = Some(content),
                (tag, content) if tag == *b"INDX" => index = Some(content),
                _ => {}
            }
        }

        let mut type_strings = None;
        let mut field_strings = None;
        let mut type_names = None;
        let mut type_bodies = None;
        for section in sections(type_sections.ok_or(DeError::MissingSection("TYPE"))?) {
            match section? {
                (tag, content) if tag == *b"TST1" => type_strings = Some(strings(content)),
                (tag, content) if tag == *b"FST1" => field_strings = Some(strings(content)),
                (tag, content) if tag == *b"TNA1" => type_names = Some(content),
                (tag, content) if tag == *b"TBDY" => type_bodies = Some(content),
                _ => {}
            }
        }
        let type_strings = type_strings.ok_or(DeError::MissingSection("TST1"))?;
        let field_strings = field_strings.ok_or(DeError::MissingSection("FST1"))?;
        let string = |strings: &[String], index: u64| {
            strings
                .get(index as usize)
                .cloned()
                .ok_or(DeError::InvalidString(index))
        };

        let mut names = Varints(type_names.ok_or(DeError::MissingSection("TNA1"))?);
        let count = usize::try_from(names.next()?).map_err(|_| DeError::InvalidSection)?;
        if count > names.0.len() || count > 16_384 {
            return Err(DeError::InvalidSection);
        }
        let mut types: Vec<Type> = (0..count).map(|_| Type::default()).collect();
        for ty in types.iter_mut().skip(1) {
            ty.name = string(&type_strings, names.next()?)?;
            // Template arguments: we only need the types' layouts, which do not use them.
            for _ in 0..names.next()? {
                names.next()?;
                names.next()?;
            }
        }

        let mut bodies = Varints(type_bodies.ok_or(DeError::MissingSection("TBDY"))?);
        while !bodies.0.is_empty() {
            let id = bodies.next()? as usize;
            if id == 0 {
                continue;
            }
            let parent = bodies.next()? as usize;
            let flags = bodies.next()?;
            let ty = types.get_mut(id).ok_or(DeError::InvalidType(id))?;
            ty.parent = parent;
            if flags & 0x01 != 0 {
                ty.format = Some(bodies.next()? as u32);
            }
            if flags & 0x02 != 0
                && ty
                    .format
                    .is_some_and(|format| format & 0xF >= format::POINTER)
            {
                ty.pointee = bodies.next()? as usize;
            }
            if flags & 0x04 != 0 {
                bodies.next()?; // Version
            }
            if flags & 0x08 != 0 {
                ty.size = Some(bodies.next()? as usize);
                bodies.next()?; // Alignment
            }
            if flags & 0x10 != 0 {
                bodies.next()?; // Abstract value
            }
            if flags & 0x20 != 0 {
                for _ in 0..bodies.next()? & 0xFFFF {
                    let name = string(&field_strings, bodies.next()?)?;
                    bodies.next()?; // Member flags
                    let offset = bodies.next()? as usize;
                    let ty = bodies.next()? as usize;
                    types[id].members.push(Member { name, offset, ty });
                }
            }
            if flags & 0x40 != 0 {
                for _ in 0..bodies.next()? {
                    bodies.next()?; // Interface type
                    bodies.next()?; // Its value
                }
            }
            if flags & 0x80 != 0 {
                bodies.next()?; // Attribute
            }
        }

        let mut items = Vec::new();
        for section in sections(index.ok_or(DeError::MissingSection("INDX"))?) {
            let (tag, content) = section?;
            if tag != *b"ITEM" {
                continue;
            }
            for item in content.as_chunks::<12>().0 {
                let word =
                    |i: usize| u32::from_le_bytes(item[i..i + 4].try_into().unwrap()) as usize;
                items.push(Item {
                    ty: word(0) & 0xFFFFFF,
                    single: word(0) >> 24 & 0x10 != 0,
                    offset: word(4),
                    count: word(8),
                });
            }
        }

        Ok(Self {
            data: data.ok_or(DeError::MissingSection("DATA"))?.to_vec(),
            types,
            items,
        })
    }

    /// The root object, the first item's.
    pub fn root(&self) -> Option<Node<'_>> {
        self.item(1)?.into_iter().next()
    }

    /// An item's elements, a single one for an object; none for the null item 0.
    fn item(&self, index: usize) -> Option<Elements<'_>> {
        let item = self.items.get(index).filter(|_| index != 0)?;
        let count = if item.single { 1 } else { item.count };
        Some(Elements {
            file: self,
            ty: item.ty,
            offset: item.offset,
            count,
        })
    }

    fn ty(&self, id: usize) -> Option<&Type> {
        self.types.get(id).filter(|_| id != 0)
    }

    /// A type's format, inherited from its parents.
    fn format(&self, mut id: usize) -> u32 {
        for _ in 0..64 {
            let Some(ty) = self.ty(id) else { break };
            match ty.format {
                Some(format) => return format,
                None => id = ty.parent,
            }
        }
        0
    }

    /// A type's size, inherited from its parents.
    fn size(&self, mut id: usize) -> usize {
        for _ in 0..64 {
            let Some(ty) = self.ty(id) else { break };
            match ty.size {
                Some(size) => return size,
                None => id = ty.parent,
            }
        }
        0
    }

    /// What a pointer or array type points to, inherited from its parents.
    fn pointee(&self, mut id: usize) -> usize {
        for _ in 0..64 {
            let Some(ty) = self.ty(id) else { break };
            match ty.pointee {
                0 => id = ty.parent,
                pointee => return pointee,
            }
        }
        0
    }

    /// A record's member by name, its own or one of its parents'.
    fn member(&self, mut id: usize, name: &str) -> Option<&Member> {
        for _ in 0..64 {
            let Some(ty) = self.ty(id) else { break };
            if let Some(member) = ty.members.iter().find(|member| member.name == name) {
                return Some(member);
            }
            id = ty.parent;
        }
        None
    }

    fn bytes<const N: usize>(&self, offset: usize) -> Option<[u8; N]> {
        self.data
            .get(offset..offset.checked_add(N)?)?
            .try_into()
            .ok()
    }
}

/// A value in a tagfile: an object, a member of one, or an array's element.
#[derive(Clone, Copy)]
pub struct Node<'a> {
    file: &'a Tagfile,
    ty: usize,
    offset: usize,
}

impl<'a> Node<'a> {
    pub fn type_name(&self) -> &'a str {
        self.file.ty(self.ty).map_or("", |ty| &ty.name)
    }

    /// A record's member.
    pub fn get(&self, name: &str) -> Option<Node<'a>> {
        let member = self.file.member(self.ty, name)?;
        Some(Node {
            file: self.file,
            ty: member.ty,
            offset: self.offset.checked_add(member.offset)?,
        })
    }

    /// What a pointer points to, typed as the object it points to.
    pub fn deref(&self) -> Option<Node<'a>> {
        // Some pointers, like `hkRefPtr`, are records wrapping the pointer.
        let node = self.unwrap_record();
        if node.file.format(node.ty) & 0xF != format::POINTER {
            return None;
        }
        node.file.item(node.index()?)?.into_iter().next()
    }

    /// The elements of an array, held inline or as an item.
    pub fn elements(&self) -> Option<Elements<'a>> {
        let format = self.file.format(self.ty);
        if format & 0xF != format::ARRAY {
            return None;
        }
        if format & format::FIXED_ARRAY != 0 {
            Some(Elements {
                file: self.file,
                ty: self.file.pointee(self.ty),
                offset: self.offset,
                count: (format >> 8) as usize,
            })
        } else {
            self.file.item(self.index()?).or(Some(Elements {
                file: self.file,
                ty: 0,
                offset: 0,
                count: 0,
            }))
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        let format = self.file.format(self.ty);
        if format & 0xF != format::INT {
            return None;
        }
        let signed = format & format::SIGNED != 0;
        Some(match self.file.size(self.ty) {
            1 if signed => i8::from_le_bytes(self.file.bytes(self.offset)?) as i64,
            1 => u8::from_le_bytes(self.file.bytes(self.offset)?) as i64,
            2 if signed => i16::from_le_bytes(self.file.bytes(self.offset)?) as i64,
            2 => u16::from_le_bytes(self.file.bytes(self.offset)?) as i64,
            4 if signed => i32::from_le_bytes(self.file.bytes(self.offset)?) as i64,
            4 => u32::from_le_bytes(self.file.bytes(self.offset)?) as i64,
            8 => i64::from_le_bytes(self.file.bytes(self.offset)?),
            _ => return None,
        })
    }

    pub fn as_f32(&self) -> Option<f32> {
        if self.file.format(self.ty) & 0xF != format::FLOAT || self.file.size(self.ty) != 4 {
            return None;
        }
        Some(f32::from_le_bytes(self.file.bytes(self.offset)?))
    }

    pub fn as_str(&self) -> Option<&'a str> {
        // Some strings, like `hkStringPtr`, are records wrapping the string.
        let node = self.unwrap_record();
        if node.file.format(node.ty) & 0xF != format::STRING {
            return None;
        }
        let item = node.file.items.get(node.index()?)?;
        let bytes = node
            .file
            .data
            .get(item.offset..item.offset.checked_add(item.count)?)?;
        std::str::from_utf8(bytes.strip_suffix(b"\0").unwrap_or(bytes)).ok()
    }

    /// Floats, as many as there are elements, in an array of floats or in a vector.
    pub fn as_f32s<const N: usize>(&self) -> Option<[f32; N]> {
        let elements = self.unwrap_record().elements()?;
        let mut floats = [0.0; N];
        for (float, element) in floats.iter_mut().zip(elements) {
            *float = element.as_f32()?;
        }
        Some(floats)
    }

    /// The single member of a record, as some types wrap a pointer, a string or an
    /// array; else this node itself.
    fn unwrap_record(&self) -> Node<'a> {
        let mut node = *self;
        for _ in 0..64 {
            if node.file.format(node.ty) & 0xF != format::RECORD {
                break;
            }
            match node.file.ty(node.ty).map(|ty| &ty.members[..]) {
                Some([member]) => {
                    node = Node {
                        file: node.file,
                        ty: member.ty,
                        offset: node.offset.saturating_add(member.offset),
                    }
                }
                _ => break,
            }
        }
        node
    }

    /// The item index a pointer, string or array holds.
    fn index(&self) -> Option<usize> {
        Some(u32::from_le_bytes(self.file.bytes(self.offset)?) as usize)
    }
}

/// An array's elements.
#[derive(Clone, Copy)]
pub struct Elements<'a> {
    file: &'a Tagfile,
    ty: usize,
    offset: usize,
    count: usize,
}

impl<'a> Elements<'a> {
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, index: usize) -> Option<Node<'a>> {
        let size = self.file.size(self.ty);
        if index >= self.count || size == 0 {
            return None;
        }
        let offset = self.offset.checked_add(index.checked_mul(size)?)?;
        self.file.data.get(offset..offset.checked_add(size)?)?;
        Some(Node {
            file: self.file,
            ty: self.ty,
            offset,
        })
    }
}

impl<'a> IntoIterator for Elements<'a> {
    type Item = Node<'a>;
    type IntoIter = ElementsIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        ElementsIter {
            elements: self,
            index: 0,
        }
    }
}

pub struct ElementsIter<'a> {
    elements: Elements<'a>,
    index: usize,
}

impl<'a> Iterator for ElementsIter<'a> {
    type Item = Node<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let node = self.elements.get(self.index)?;
        self.index += 1;
        Some(node)
    }
}

/// The sections in `bytes`, each as its tag and its content.
fn sections(mut bytes: &[u8]) -> impl Iterator<Item = Result<([u8; 4], &[u8]), DeError>> {
    std::iter::from_fn(move || {
        if bytes.is_empty() {
            return None;
        }
        let Some(header) = bytes.get(..8) else {
            bytes = &[];
            return Some(Err(DeError::InvalidSection));
        };
        Some(
            (|| {
                let size =
                    (u32::from_be_bytes(header[..4].try_into().unwrap()) & 0x3FFFFFFF) as usize;
                let tag = header[4..8].try_into().unwrap();
                let content = bytes.get(8..size).ok_or(DeError::InvalidSection)?;
                bytes = &bytes[size..];
                Ok((tag, content))
            })()
            .inspect_err(|_| bytes = &[]),
        )
    })
}

/// The null-terminated strings of a strings section.
fn strings(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&b| b == 0)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// The variable-length integers of a types section: the first byte's high bits give the
/// length, its low bits and the following bytes the big-endian value.
struct Varints<'a>(&'a [u8]);

impl Varints<'_> {
    fn next(&mut self) -> Result<u64, DeError> {
        let &first = self.0.first().ok_or(DeError::InvalidVarint)?;
        let (len, mask) = match first {
            0x00..=0x7F => (1, 0x7F),
            0x80..=0xBF => (2, 0x3FFF),
            0xC0..=0xDF => (3, 0x1FFFFF),
            0xE0..=0xE7 => (4, 0x7FFFFFF),
            0xE8..=0xEF => (5, 0x7FFFFFFFF),
            0xF0..=0xF7 => (6, 0x7FFFFFFFFFF),
            _ => (9, u64::MAX),
        };
        let bytes = self.0.get(..len).ok_or(DeError::InvalidVarint)?;
        // The 9-byte form's value is in the 8 bytes after the first.
        let value = bytes[usize::from(len == 9)..]
            .iter()
            .fold(0u64, |value, &b| value << 8 | u64::from(b));
        self.0 = &self.0[len..];
        Ok(value & mask)
    }
}

/// Errors that can happen while reading a tagfile.
#[derive(Debug, Error)]
pub enum DeError {
    #[error("not a tagfile")]
    InvalidMagic,
    #[error("a section overflows its parent")]
    InvalidSection,
    #[error("the {0} section is missing")]
    MissingSection(&'static str),
    #[error("a variable-length integer is cut")]
    InvalidVarint,
    #[error("no string {0}")]
    InvalidString(u64),
    #[error("no type {0}")]
    InvalidType(usize),
}

use anyhow::{Context, Result, bail, ensure};

#[derive(Debug)]
pub struct Section {
    pub address: u64,
    pub file_offset: usize,
    pub file_size: usize,
    pub executable: bool,
    pub writable: bool,
}

pub struct Image<'a> {
    pub bytes: &'a [u8],
    pub base: u64,
    pub sections: Vec<Section>,
    pub functions: Vec<(u64, u64)>,
}

fn read<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}

impl<'a> Image<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        ensure!(bytes.starts_with(b"MZ"), "expected a Windows PE executable");
        let pe = u32::from_le_bytes(read(bytes, 0x3c).context("truncated DOS header")?) as usize;
        ensure!(
            bytes.get(pe..pe + 4) == Some(b"PE\0\0"),
            "invalid PE signature"
        );
        let word = |o| {
            read(bytes, o)
                .map(u16::from_le_bytes)
                .context("truncated PE header")
        };
        let dword = |o| {
            read(bytes, o)
                .map(u32::from_le_bytes)
                .context("truncated PE header")
        };
        ensure!(
            word(pe + 4)? == 0x8664,
            "only AMD64 executables are supported"
        );
        let section_count = word(pe + 6)? as usize;
        let optional_size = word(pe + 20)? as usize;
        let optional = pe + 24;
        ensure!(
            optional_size >= 144 && word(optional)? == 0x20b,
            "expected PE32+ headers"
        );
        let base = u64::from_le_bytes(read(bytes, optional + 24).context("missing image base")?);
        let mut sections = Vec::new();
        for index in 0..section_count {
            let offset = optional + optional_size + index * 40;
            let name = read::<8>(bytes, offset).context("truncated section table")?;
            let name = String::from_utf8_lossy(&name)
                .trim_end_matches('\0')
                .to_owned();
            let address = base
                .checked_add(dword(offset + 12)? as u64)
                .context("section VA overflow")?;
            let file_size = dword(offset + 16)? as usize;
            let file_offset = dword(offset + 20)? as usize;
            ensure!(
                file_offset
                    .checked_add(file_size)
                    .is_some_and(|end| end <= bytes.len()),
                "section {name} exceeds file bounds"
            );
            let flags = dword(offset + 36)?;
            sections.push(Section {
                address,
                file_offset,
                file_size,
                executable: flags & 0x2000_0000 != 0,
                writable: flags & 0x8000_0000 != 0,
            });
        }
        let mut image = Self {
            bytes,
            base,
            sections,
            functions: Vec::new(),
        };
        let count = dword(optional + 108)?;
        if count > 3 {
            let rva = dword(optional + 112 + 3 * 8)? as u64;
            let size = dword(optional + 116 + 3 * 8)? as usize;
            let table = image
                .slice(base + rva, size)
                .context("invalid exception directory")?;
            ensure!(size.is_multiple_of(12), "misaligned exception directory");
            for entry in table.as_chunks::<12>().0 {
                let start = base + u32::from_le_bytes(entry[..4].try_into()?) as u64;
                let end = base + u32::from_le_bytes(entry[4..8].try_into()?) as u64;
                if start >= end || image.slice(start, (end - start) as usize).is_none() {
                    bail!("invalid runtime function range");
                }
                image.functions.push((start, end));
            }
            image.functions.sort_unstable();
        }
        Ok(image)
    }

    pub fn slice(&self, address: u64, size: usize) -> Option<&'a [u8]> {
        self.sections.iter().find_map(|section| {
            let relative = usize::try_from(address.checked_sub(section.address)?).ok()?;
            if relative.checked_add(size)? > section.file_size {
                return None;
            }
            self.bytes
                .get(section.file_offset + relative..section.file_offset + relative + size)
        })
    }

    pub fn u32(&self, address: u64) -> Option<u32> {
        Some(u32::from_le_bytes(self.slice(address, 4)?.try_into().ok()?))
    }
    pub fn u64(&self, address: u64) -> Option<u64> {
        Some(u64::from_le_bytes(self.slice(address, 8)?.try_into().ok()?))
    }
    pub fn string(&self, address: u64) -> Option<String> {
        let section = self
            .sections
            .iter()
            .find(|s| address >= s.address && address < s.address + s.file_size as u64)?;
        let length = (section.address + section.file_size as u64 - address).min(4096) as usize;
        let bytes = self.slice(address, length)?;
        let end = bytes.iter().position(|b| *b == 0)?;
        let bytes = &bytes[..end];
        if bytes.is_empty() || !bytes.iter().all(|b| (32..127).contains(b)) {
            return None;
        }
        String::from_utf8(bytes.to_vec()).ok()
    }
    pub fn executable(&self, address: u64) -> bool {
        self.sections.iter().any(|s| {
            s.executable && address >= s.address && address < s.address + s.file_size as u64
        })
    }
}

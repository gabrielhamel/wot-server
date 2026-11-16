//! Package file codec.
//!
//! Packages are ZIP files with constrained flags and properties,
//! for example no encryption and no compression is needed.
//!
//! Following official specification:
//! <https://pkware.cachefly.net/webdocs/casestudies/APPNOTE.TXT>

use std::io::{self, BufReader, Read, Seek, SeekFrom};

use byteorder::{LE, ReadBytesExt};

/// Signature for the Local File Header structure.
const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x04034b50;

/// Signature for the Central Directory Header structure.
const CENTRAL_DIRECTORY_HEADER_SIGNATURE: u32 = 0x02014b50;

/// Signature for the end of central directory.
const END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x06054b50;

/// A package-specialized ZIP reader that is optimized for reading all file names as fast
/// as possible. This reader only accesses file immutably. This reader ignores folders.
///
/// If the underlying stream (such as file) is modified while this reader is create,
/// subsequent file reads may fail.
pub struct PackageReader<R: Read + Seek> {
    /// Underlying reader. Not buffered because once the header has been parsed, the data
    /// reading will be spread way over the default 8 KB block of the buffered reader,
    /// so this is useless.
    inner: R,
    /// This string buffer holds all file names, so only one allocation is needed for all
    /// names.
    name_buffer: String,
    /// File metadata in central-directory order.
    file_infos: Vec<PackageFileInternalInfo>,
}

/// Internal metadata about a file.
#[derive(Debug)]
struct PackageFileInternalInfo {
    /// Offset of the package name into the global name buffer.
    name_offset: usize,
    /// Length of the file name in the global name buffer.
    name_len: u16,
    /// Offset within the file of the local header of this file.
    header_offset: u32,
    /// Expected uncompressed size for this file, packages should not compress files
    /// so the compressed size should be equal, but this will be checked later if the
    /// file is actually opened.
    size: u32,
}

impl<R: Read + Seek> PackageReader<R> {
    /// Create a package reader with the underlying read+seek implementor.
    pub fn new(mut reader: R) -> io::Result<Self> {
        const HEADER_MIN_SIZE: u64 = 22;
        const HEADER_MAX_SIZE: u64 = 22 + u16::MAX as u64;

        // Here we try to find the position of the End of Central Directory.
        let file_length = reader.seek(SeekFrom::End(0))?;
        let mut eocd_pos = file_length
            .checked_sub(HEADER_MIN_SIZE)
            .ok_or(io::Error::new(
                io::ErrorKind::InvalidData,
                "cannot find end of central directory, file too short",
            ))?;
        let eocd_pos_bound = file_length.saturating_sub(HEADER_MAX_SIZE);

        // A successful return from this loop means we found the EoCD position.
        loop {
            reader.seek(SeekFrom::Start(eocd_pos))?;
            if reader.read_u32::<LE>()? == END_OF_CENTRAL_DIRECTORY_SIGNATURE {
                break;
            }

            if eocd_pos == eocd_pos_bound {
                // If we didn't find signature on the lower bound.
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "cannot find end of central directory, reached lower bound",
                ));
            }

            eocd_pos = eocd_pos.checked_sub(1).ok_or(io::Error::new(
                io::ErrorKind::InvalidData,
                "cannot find end of central directory, underflow position",
            ))?;
        }

        // Here we finish parsing the EoCD (we are placed just after the directory signature).
        let disk_number = reader.read_u16::<LE>()?;
        let disk_with_central_directory = reader.read_u16::<LE>()?;

        if disk_number != disk_with_central_directory {
            // Multi-disk ZIP files are not valid packages.
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "multi-disk is not supported for packages zip",
            ));
        }

        let number_of_files_on_this_disk = reader.read_u16::<LE>()?;
        let number_of_files = reader.read_u16::<LE>()?;

        if number_of_files_on_this_disk != number_of_files {
            // Same as above, no multi-disk, so the number of files must be coherent.
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "multi-disk is not supported for packages zip",
            ));
        }

        let _central_directory_size = reader.read_u32::<LE>()?;
        let central_directory_offset = reader.read_u32::<LE>()?;

        let comment_length = reader.read_u16::<LE>()?;
        if comment_length != 0 {
            // Not expecting comments on packages.
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "comments are not supported for packages zip",
            ));
        }

        // Now we can start parsing all Central Directory Headers.
        // Seek to the first Central Directory Header, reading is ready.
        reader.seek(SeekFrom::Start(central_directory_offset as u64))?;

        // For decoding the package structure we use a buffered reader to optimize
        // our random reads.
        let mut reader = BufReader::new(reader);

        // At start, we only read file names and optimize their storage, the actual file
        // header, size, flags will be read only when the file is accessed, here we only
        // read file name and store the offset header.
        // On average in World of Tanks packages, there is 70 bytes per file name.
        let mut name_buffer = Vec::with_capacity(number_of_files as usize * 70);
        let mut file_infos = Vec::with_capacity(number_of_files as usize);

        for _ in 0..number_of_files {
            if reader.read_u32::<LE>()? != CENTRAL_DIRECTORY_HEADER_SIGNATURE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid central directory header signature",
                ));
            }

            // Skip most of the header that we don't care at this point.
            reader.seek_relative(20)?;
            // Uncompressed size is used as
            let uncompressed_size = reader.read_u32::<LE>()?;
            // Then we read all variable lengths.
            let file_name_len = reader.read_u16::<LE>()?;
            // Read both fields at once because we want ot check that it's zero.
            let extra_field_file_comment_len = reader.read_u32::<LE>()?;
            // Skip again, disk num, file attrs.
            reader.seek_relative(8)?;
            // Then read the offset of the local file header.
            let relative_offset = reader.read_u32::<LE>()?;

            // Extra field and comment are not supported nor used by Wargaming.
            if extra_field_file_comment_len != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "comments on files are not supported for packages zip",
                ));
            }

            // Start by increasing the buffer capacity.
            let name_offset = name_buffer.len();
            name_buffer.resize(name_buffer.len() + file_name_len as usize, 0);
            let this_name_buffer = &mut name_buffer[name_offset..][..file_name_len as usize];
            reader.read_exact(this_name_buffer)?;

            // If the name buffer is empty or ends with a slash, just ignore that because
            // it's a folder and don't keep folders. We rollback changes to name buffer
            // and continue on next iteration.
            if let None | Some(b'/') = this_name_buffer.last() {
                name_buffer.truncate(name_offset);
                continue;
            }

            std::str::from_utf8(this_name_buffer).map_err(io::Error::other)?;

            // Push the metadata to the files array.
            file_infos.push(PackageFileInternalInfo {
                name_offset,
                name_len: file_name_len,
                header_offset: relative_offset,
                size: uncompressed_size,
            });
        }

        let name_buffer = String::from_utf8(name_buffer).map_err(io::Error::other)?;

        Ok(Self {
            inner: reader.into_inner(),
            name_buffer,
            file_infos,
        })
    }

    /// Return an iterator over all file info in the package. The position of files in
    /// this iterator is their index that can be used when reading from index, using
    /// the [`Self::read_by_index()`] method.
    pub fn infos(&self) -> impl Iterator<Item = PackageFileInfo<'_>> {
        self.file_infos.iter().map(|info| PackageFileInfo {
            name: &self.name_buffer[info.name_offset..][..info.name_len as usize],
            size: info.size,
        })
    }

    /// Open a package file by its index and return a borrowed reader if successful.
    ///
    /// Note that the returned reader has no buffered over the original reader given at
    /// construction, you should handle buffering if necessary.
    pub fn read_by_index(&mut self, file_index: usize) -> io::Result<PackageFileReader<&'_ mut R>> {
        let info = self
            .file_infos
            .get(file_index)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;

        // Start to the start of the header.
        self.inner
            .seek(SeekFrom::Start(info.header_offset as u64))?;
        if self.inner.read_u32::<LE>()? != LOCAL_FILE_HEADER_SIGNATURE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid local file header signature",
            ));
        }

        // Skip version needed to extract.
        self.inner.seek(SeekFrom::Current(2))?;
        let flags = self.inner.read_u16::<LE>()?;
        let compression_method = self.inner.read_u16::<LE>()?;
        // Skip file time/date/crc32
        self.inner.seek(SeekFrom::Current(2 + 2 + 4))?;
        let compressed_size = self.inner.read_u32::<LE>()?;
        let uncompressed_size = self.inner.read_u32::<LE>()?;
        let name_len = self.inner.read_u16::<LE>()?;
        let extra_len = self.inner.read_u16::<LE>()?;
        if name_len != info.name_len || extra_len != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid local file header lengths",
            ));
        }
        self.inner.seek(SeekFrom::Current(i64::from(name_len)))?;

        // Incoherent uncompressed size, different from central directory header!
        if uncompressed_size != info.size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "incoherent size in local file header vs central directory header: {uncompressed_size} != {}",
                    info.size
                ),
            ));
        }

        // Packages don't compress files.
        if compression_method != 0 || compressed_size != uncompressed_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "compressed files are not valid for packages zip: {compressed_size} (compressed) != {uncompressed_size} (uncompressed)"
                ),
            ));
        }

        // Packages has no flag, no delayed crc32/size, no compression, no encryption.
        // We mask the bit 11, we don't know why it is used but apparently it can mean
        // to decode the filename/comment using UTF-8. So yes, we already do that.
        if flags & !2048 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("flags on files are not valid for packages zip: {flags:016b}"),
            ));
        }

        // Now the reader's cursor is at data start, return the file reader.
        Ok(PackageFileReader {
            inner: &mut self.inner,
            remaining_len: compressed_size,
        })
    }
}

/// Information about a package file that can be read.
#[derive(Debug, Clone)]
pub struct PackageFileInfo<'a> {
    /// The file name that should be used when reading.
    pub name: &'a str,
    /// The size of this file when read.
    pub size: u32,
}

/// A handle for reading a file in a package.
#[derive(Debug)]
pub struct PackageFileReader<R: Read + Seek> {
    /// Underlying reader.
    inner: R,
    /// Remaining length to read from the file.
    remaining_len: u32,
}

impl<R: Read + Seek> Read for PackageFileReader<R> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // If remaining length is zero, this will just do nothing.
        let len = buf.len().min(self.remaining_len as usize);
        let len = self.inner.read(&mut buf[..len])?;
        self.remaining_len -= len as u32;
        Ok(len)
    }

    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        if (self.remaining_len as usize) < buf.len() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.inner.read_exact(buf)?;
        self.remaining_len -= buf.len() as u32;
        Ok(())
    }
}

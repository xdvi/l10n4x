/// The current supported format version of the binary package format.
pub const FORMAT_VERSION: u32 = 1;

use crate::error::CoreResult;

/// High-performance parser and reader for the custom binary `.pak` format.
/// Performs O(log N) binary search lookups on alphabetical keys directly from
/// read-only decompressed memory buffers, avoiding copies and allocations.
pub struct BinaryFormatReader<'a> {
    data: &'a [u8],
}

impl<'a> BinaryFormatReader<'a> {
    /// Instantiates a new reader from a raw byte slice.
    /// Validates the buffer length, magic bytes `"L10N"`, and the format version.
    pub fn new(data: &'a [u8]) -> CoreResult<Self> {
        if data.len() < 16 {
            return Err(crate::CoreError::InvalidFormat("Invalid buffer length"));
        }
        if &data[0..4] != b"L10N" {
            return Err(crate::CoreError::InvalidMagic("Invalid magic bytes"));
        }
        let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
        if version != FORMAT_VERSION {
            return Err(crate::CoreError::UnsupportedVersion(version));
        }
        Ok(Self { data })
    }

    /// Performs binary search lookup on the sorted index to locate the bytecode of a key.
    /// Returns `Some(&[u8])` representing the bytecode slice if found, or `None` otherwise.
    pub fn lookup(&self, key: &str) -> Option<&'a [u8]> {
        let index_offset = u32::from_be_bytes(self.data[8..12].try_into().unwrap()) as usize;
        let index_count = u32::from_be_bytes(self.data[12..16].try_into().unwrap()) as usize;

        let mut low = 0;
        let mut high = index_count;

        while low < high {
            let mid = (low + high) / 2;
            let entry_offset = index_offset + mid * 16;
            if entry_offset + 16 > self.data.len() {
                return None;
            }

            let key_offset = u32::from_be_bytes(
                self.data[entry_offset..entry_offset + 4]
                    .try_into()
                    .unwrap(),
            ) as usize;
            let key_len = u32::from_be_bytes(
                self.data[entry_offset + 4..entry_offset + 8]
                    .try_into()
                    .unwrap(),
            ) as usize;

            if key_offset + key_len > self.data.len() {
                return None;
            }

            if let Ok(entry_key) =
                core::str::from_utf8(&self.data[key_offset..key_offset + key_len])
            {
                match entry_key.cmp(key) {
                    core::cmp::Ordering::Equal => {
                        let val_offset = u32::from_be_bytes(
                            self.data[entry_offset + 8..entry_offset + 12]
                                .try_into()
                                .unwrap(),
                        ) as usize;
                        let val_len = u32::from_be_bytes(
                            self.data[entry_offset + 12..entry_offset + 16]
                                .try_into()
                                .unwrap(),
                        ) as usize;
                        if val_offset + val_len > self.data.len() {
                            return None;
                        }
                        return Some(&self.data[val_offset..val_offset + val_len]);
                    }
                    core::cmp::Ordering::Less => {
                        low = mid + 1;
                    }
                    core::cmp::Ordering::Greater => {
                        high = mid;
                    }
                }
            } else {
                return None;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn create_minimal_binary() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&16u32.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf
    }

    #[test]
    fn new_accepts_valid_header() {
        let data = create_minimal_binary();
        assert!(BinaryFormatReader::new(&data).is_ok());
    }

    #[test]
    fn new_rejects_short_buffer() {
        let result = BinaryFormatReader::new(b"");
        assert!(result.is_err());
    }

    #[test]
    fn new_rejects_bad_magic() {
        let data = b"XXXX\x00\x00\x00\x01\x00\x00\x00\x10\x00\x00\x00\x00";
        let result = BinaryFormatReader::new(data);
        assert!(result.is_err());
    }

    #[test]
    fn lookup_returns_none_for_empty_store() {
        let data = create_minimal_binary();
        let reader = BinaryFormatReader::new(&data).unwrap();
        assert!(reader.lookup("anything").is_none());
    }

    #[test]
    fn lookup_returns_none_for_missing_key() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        let index_offset: u32 = 16;
        buf.extend_from_slice(&index_offset.to_be_bytes());
        let index_count: u32 = 1;
        buf.extend_from_slice(&index_count.to_be_bytes());
        let key = b"a";
        let val = b"x";
        let key_off = 16 + 16;
        let val_off = key_off + key.len() as u32;
        buf.extend_from_slice(&key_off.to_be_bytes());
        buf.extend_from_slice(&(key.len() as u32).to_be_bytes());
        buf.extend_from_slice(&val_off.to_be_bytes());
        buf.extend_from_slice(&(val.len() as u32).to_be_bytes());
        buf.extend_from_slice(key);
        buf.extend_from_slice(val);
        let reader = BinaryFormatReader::new(&buf).unwrap();
        assert!(reader.lookup("a").is_some());
        assert!(reader.lookup("missing").is_none());
    }

    #[test]
    fn new_rejects_unsupported_version() {
        let mut buf = create_minimal_binary();
        buf[4..8].copy_from_slice(&99u32.to_be_bytes());
        let result = BinaryFormatReader::new(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn lookup_corrupted_key_offset_returns_none() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&16u32.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        // key_offset = 9999, key_len = 1 → out of bounds
        buf.extend_from_slice(&9999u32.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&16u32.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        let reader = BinaryFormatReader::new(&buf).unwrap();
        assert!(reader.lookup("x").is_none());
    }

    #[test]
    fn lookup_corrupted_val_offset_returns_none() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&16u32.to_be_bytes());
        buf.extend_from_slice(&1u32.to_be_bytes());
        // key data
        let key_data = [b'a'];
        buf.extend_from_slice(&32u32.to_be_bytes()); // key_offset → key data at 32
        buf.extend_from_slice(&1u32.to_be_bytes()); // key_len = 1
        buf.extend_from_slice(&9999u32.to_be_bytes()); // val_offset = 9999 (corrupted)
        buf.extend_from_slice(&1u32.to_be_bytes()); // val_len = 1
        buf.push(key_data[0]);
        let reader = BinaryFormatReader::new(&buf).unwrap();
        assert!(reader.lookup("a").is_none());
    }
}

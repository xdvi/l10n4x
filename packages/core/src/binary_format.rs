/// The current supported format version of the binary package format.
pub const FORMAT_VERSION: u32 = 1;

/// FNV-1a 64-bit hash for translation keys. Deterministic, fast, collision-free.
pub fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

use crate::error::CoreResult;

/// High-performance parser and reader for the custom binary `.pak` format.
/// Performs O(log N) binary search lookups on u64 FNV-1a hashed keys directly from
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

    /// Performs binary search on the sorted u64 hash index to locate the bytecode.
    /// Each index entry is 16 bytes: [hash:8B][val_offset:4B][val_len:4B].
    pub fn lookup(&self, key_hash: u64) -> Option<&'a [u8]> {
        let index_offset = u32::from_be_bytes(self.data[8..12].try_into().unwrap()) as usize;
        let index_count = u32::from_be_bytes(self.data[12..16].try_into().unwrap()) as usize;
        let entry_size = 16usize;

        let mut low = 0;
        let mut high = index_count;

        while low < high {
            let mid = (low + high) / 2;
            let entry_offset = index_offset + mid * entry_size;
            if entry_offset + entry_size > self.data.len() {
                return None;
            }
            let hash = u64::from_be_bytes(
                self.data[entry_offset..entry_offset + 8].try_into().unwrap(),
            );

            match hash.cmp(&key_hash) {
                core::cmp::Ordering::Equal => {
                    let val_offset = u32::from_be_bytes(
                        self.data[entry_offset + 8..entry_offset + 12].try_into().unwrap(),
                    ) as usize;
                    let val_len = u32::from_be_bytes(
                        self.data[entry_offset + 12..entry_offset + 16].try_into().unwrap(),
                    ) as usize;
                    if val_offset + val_len > self.data.len() {
                        return None;
                    }
                    return Some(&self.data[val_offset..val_offset + val_len]);
                }
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn hash(s: &str) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

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
        assert!(reader.lookup(hash("anything")).is_none());
    }

    #[test]
    fn lookup_returns_none_for_missing_key() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        let index_offset: u32 = 16 + 1; // header + value data
        buf.extend_from_slice(&index_offset.to_be_bytes());
        let index_count: u32 = 1;
        buf.extend_from_slice(&index_count.to_be_bytes());
        let val = b"x";
        buf.extend_from_slice(val);
        let hash_a = hash("a");
        buf.extend_from_slice(&hash_a.to_be_bytes());
        buf.extend_from_slice(&16u32.to_be_bytes()); // val_offset
        buf.extend_from_slice(&(val.len() as u32).to_be_bytes());
        let reader = BinaryFormatReader::new(&buf).unwrap();
        assert!(reader.lookup(hash_a).is_some());
        assert!(reader.lookup(hash("missing")).is_none());
    }

    #[test]
    fn new_rejects_unsupported_version() {
        let mut buf = create_minimal_binary();
        buf[4..8].copy_from_slice(&99u32.to_be_bytes());
        let result = BinaryFormatReader::new(&buf);
        assert!(result.is_err());
    }

    #[test]
    fn lookup_corrupted_val_offset_returns_none() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&(16u32 + 16u32).to_be_bytes()); // index_offset
        buf.extend_from_slice(&1u32.to_be_bytes());
        // value data at 16
        buf.push(b'x');
        // index: [hash:8B][val_offset:4B][val_len:4B]
        let h = hash("a");
        buf.extend_from_slice(&h.to_be_bytes());
        buf.extend_from_slice(&9999u32.to_be_bytes()); // corrupted val_offset
        buf.extend_from_slice(&1u32.to_be_bytes());
        let reader = BinaryFormatReader::new(&buf).unwrap();
        assert!(reader.lookup(h).is_none());
    }

    #[test]
    fn lookup_corrupted_val_len_returns_none() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&(16u32 + 16u32).to_be_bytes()); // index_offset
        buf.extend_from_slice(&1u32.to_be_bytes());
        // value data at 16
        buf.push(b'x');
        // index: [hash:8B][val_offset:4B][val_len:4B]
        let h = hash("a");
        buf.extend_from_slice(&h.to_be_bytes());
        buf.extend_from_slice(&16u32.to_be_bytes()); // val_offset = correct
        buf.extend_from_slice(&9999u32.to_be_bytes()); // val_len = 9999 (corrupted, out of bounds)
        let reader = BinaryFormatReader::new(&buf).unwrap();
        assert!(reader.lookup(h).is_none());
    }
}

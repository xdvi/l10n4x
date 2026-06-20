/// The current supported format version of the binary package format.
pub const FORMAT_VERSION: u32 = 1;

/// High-performance parser and reader for the custom binary `.pak` format.
/// Performs O(log N) binary search lookups on alphabetical keys directly from
/// read-only decrypted memory buffers, avoiding copies and allocations.
pub struct BinaryFormatReader<'a> {
    data: &'a [u8],
}

impl<'a> BinaryFormatReader<'a> {
    /// Instantiates a new reader from a raw byte slice.
    /// Validates the buffer length, magic bytes `"L10N"`, and the format version.
    pub fn new(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.len() < 16 {
            return Err("Invalid buffer length");
        }
        if &data[0..4] != b"L10N" {
            return Err("Invalid magic bytes");
        }
        let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
        if version != FORMAT_VERSION {
            return Err("Unsupported format version");
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

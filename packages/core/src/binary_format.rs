use crate::error::CoreResult;

#[cfg(feature = "std")]
use alloc::collections::BTreeMap;
#[cfg(feature = "std")]
use alloc::string::String;
#[cfg(feature = "std")]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use std::collections::HashMap;

/// Legacy L10N format version (16-byte header).
pub const FORMAT_VERSION_V1: u32 = 1;
/// L10N v2 (20-byte header + `min_runtime_version`).
pub const FORMAT_VERSION_V2: u32 = 2;
/// L10N v3 (24-byte header + `locale_data_version` pinning).
pub const FORMAT_VERSION_V3: u32 = 3;
/// Latest format version emitted by the compiler.
pub const FORMAT_VERSION: u32 = FORMAT_VERSION_V3;
/// Runtime API version; bump when breaking runtime behavior.
pub const RUNTIME_VERSION: u32 = 1;

const HEADER_SIZE_V1: usize = 16;
const HEADER_SIZE_V2: usize = 20;
const HEADER_SIZE_V3: usize = 24;
const INDEX_ENTRY_SIZE: usize = 16;
#[cfg(feature = "debug-keys")]
const DEBUG_SECTION_MAGIC: &[u8; 4] = b"DBGK";

/// FNV-1a 64-bit hash for translation keys. Deterministic, fast, collision-free.
pub fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn read_u32_be(data: &[u8], off: usize) -> Option<u32> {
    data.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_be_bytes)
}

/// High-performance parser and reader for the custom binary `.pak` format.
#[derive(Debug)]
pub struct BinaryFormatReader<'a> {
    data: &'a [u8],
    version: u32,
    header_size: usize,
    min_runtime_version: u32,
    locale_data_version: u32,
}

impl<'a> BinaryFormatReader<'a> {
    /// Instantiates a new reader from a raw byte slice.
    /// Accepts format v1 (legacy) and v2 (with `min_runtime_version`).
    pub fn new(data: &'a [u8]) -> CoreResult<Self> {
        if data.len() < HEADER_SIZE_V1 {
            return Err(crate::CoreError::InvalidFormat("Invalid buffer length"));
        }
        if &data[0..4] != b"L10N" {
            return Err(crate::CoreError::InvalidMagic("Invalid magic bytes"));
        }
        let version = read_u32_be(data, 4)
            .ok_or(crate::CoreError::InvalidFormat("truncated version field"))?;
        let (header_size, min_runtime_version, locale_data_version) = match version {
            FORMAT_VERSION_V1 => (HEADER_SIZE_V1, 0, 0),
            FORMAT_VERSION_V2 => {
                if data.len() < HEADER_SIZE_V2 {
                    return Err(crate::CoreError::InvalidFormat("truncated v2 header"));
                }
                let min_rt = read_u32_be(data, 8).ok_or(crate::CoreError::InvalidFormat(
                    "truncated min_runtime_version",
                ))?;
                if min_rt > RUNTIME_VERSION {
                    return Err(crate::CoreError::RuntimeTooOld {
                        required: min_rt,
                        current: RUNTIME_VERSION,
                    });
                }
                (HEADER_SIZE_V2, min_rt, 0)
            }
            FORMAT_VERSION_V3 => {
                if data.len() < HEADER_SIZE_V3 {
                    return Err(crate::CoreError::InvalidFormat("truncated v3 header"));
                }
                let min_rt = read_u32_be(data, 8).ok_or(crate::CoreError::InvalidFormat(
                    "truncated min_runtime_version",
                ))?;
                if min_rt > RUNTIME_VERSION {
                    return Err(crate::CoreError::RuntimeTooOld {
                        required: min_rt,
                        current: RUNTIME_VERSION,
                    });
                }
                let locale_data = read_u32_be(data, 12).ok_or(crate::CoreError::InvalidFormat(
                    "truncated locale_data_version",
                ))?;
                if locale_data > crate::locale_data::SUPPORTED_LOCALE_DATA_VERSION {
                    return Err(crate::CoreError::LocaleDataTooOld {
                        required: locale_data,
                        current: crate::locale_data::SUPPORTED_LOCALE_DATA_VERSION,
                    });
                }
                (HEADER_SIZE_V3, min_rt, locale_data)
            }
            other => return Err(crate::CoreError::UnsupportedVersion(other)),
        };
        Ok(Self {
            data,
            version,
            header_size,
            min_runtime_version,
            locale_data_version,
        })
    }

    /// Format version of this buffer.
    pub fn format_version(&self) -> u32 {
        self.version
    }

    /// Minimum runtime version required (0 for legacy v1 buffers).
    pub fn min_runtime_version(&self) -> u32 {
        self.min_runtime_version
    }

    /// Pinned CLDR/locale data revision (0 for legacy v1/v2 buffers).
    pub fn locale_data_version(&self) -> u32 {
        self.locale_data_version
    }

    fn index_offset(&self) -> usize {
        let off = match self.version {
            FORMAT_VERSION_V3 => 16,
            FORMAT_VERSION_V2 => 12,
            _ => 8,
        };
        read_u32_be(self.data, off).unwrap_or(0) as usize
    }

    fn index_count(&self) -> usize {
        let off = match self.version {
            FORMAT_VERSION_V3 => 20,
            FORMAT_VERSION_V2 => 16,
            _ => 12,
        };
        read_u32_be(self.data, off).unwrap_or(0) as usize
    }

    #[cfg(feature = "debug-keys")]
    fn index_end(&self) -> usize {
        self.index_offset() + self.index_count() * INDEX_ENTRY_SIZE
    }

    /// Size in bytes of the format header (16 for v1, 20 for v2).
    pub fn header_byte_len(&self) -> usize {
        self.header_size
    }

    /// Performs binary search on the sorted u64 hash index to locate the bytecode.
    pub fn lookup(&self, key_hash: u64) -> Option<&'a [u8]> {
        let index_offset = self.index_offset();
        let index_count = self.index_count();

        let mut low = 0;
        let mut high = index_count;

        while low < high {
            let mid = (low + high) / 2;
            let entry_offset = index_offset + mid * INDEX_ENTRY_SIZE;
            if entry_offset + INDEX_ENTRY_SIZE > self.data.len() {
                return None;
            }
            let hash = u64::from_be_bytes(
                self.data[entry_offset..entry_offset + 8]
                    .try_into()
                    .unwrap(),
            );

            match hash.cmp(&key_hash) {
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
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
            }
        }
        None
    }

    /// Collects all `(hash, value_bytes)` pairs from the index.
    #[cfg(feature = "std")]
    pub fn collect_entries(&self) -> Vec<(u64, Vec<u8>)> {
        let mut out = Vec::with_capacity(self.index_count());
        let index_offset = self.index_offset();
        for i in 0..self.index_count() {
            let entry_offset = index_offset + i * INDEX_ENTRY_SIZE;
            if entry_offset + INDEX_ENTRY_SIZE > self.data.len() {
                break;
            }
            let hash = u64::from_be_bytes(
                self.data[entry_offset..entry_offset + 8]
                    .try_into()
                    .unwrap(),
            );
            if let Some(val) = self.lookup(hash) {
                out.push((hash, val.to_vec()));
            }
        }
        out
    }

    /// Builds a HashMap from the sorted index.
    #[cfg(feature = "std")]
    pub fn to_offsets(&self) -> HashMap<u64, (u32, u32)> {
        let index_offset = self.index_offset();
        let index_count = self.index_count();
        let mut map = HashMap::with_capacity(index_count);
        for i in 0..index_count {
            let entry_offset = index_offset + i * INDEX_ENTRY_SIZE;
            if entry_offset + INDEX_ENTRY_SIZE > self.data.len() {
                break;
            }
            let hash = u64::from_be_bytes(
                self.data[entry_offset..entry_offset + 8]
                    .try_into()
                    .unwrap(),
            );
            let val_offset = u32::from_be_bytes(
                self.data[entry_offset + 8..entry_offset + 12]
                    .try_into()
                    .unwrap(),
            );
            let val_len = u32::from_be_bytes(
                self.data[entry_offset + 12..entry_offset + 16]
                    .try_into()
                    .unwrap(),
            );
            if (val_offset as usize) + (val_len as usize) <= self.data.len() {
                map.insert(hash, (val_offset, val_len));
            }
        }
        map
    }

    /// Parses optional `DBGK` debug key table appended after the index.
    #[cfg(feature = "debug-keys")]
    pub fn debug_key_table(&self) -> HashMap<u64, String> {
        let mut map = HashMap::new();
        let mut pos = self.index_end();
        if pos + 8 > self.data.len() {
            return map;
        }
        if &self.data[pos..pos + 4] != DEBUG_SECTION_MAGIC {
            return map;
        }
        pos += 4;
        let count = read_u32_be(self.data, pos).unwrap_or(0) as usize;
        pos += 4;
        for _ in 0..count {
            if pos + 12 > self.data.len() {
                break;
            }
            let hash = u64::from_be_bytes(self.data[pos..pos + 8].try_into().unwrap());
            pos += 8;
            let key_len = read_u32_be(self.data, pos).unwrap_or(0) as usize;
            pos += 4;
            if pos + key_len > self.data.len() {
                break;
            }
            if let Ok(key) = core::str::from_utf8(&self.data[pos..pos + key_len]) {
                map.insert(hash, key.to_string());
            }
            pos += key_len;
        }
        map
    }
}

/// Merges two L10N buffers; entries in `newer` override `existing` on hash collision.
#[cfg(feature = "std")]
pub fn merge_l10n_buffers(existing: &[u8], newer: &[u8]) -> CoreResult<Vec<u8>> {
    let left = BinaryFormatReader::new(existing)?;
    let right = BinaryFormatReader::new(newer)?;
    // Dedup-by-hash via a BTreeMap (later buffer wins), then drain into a sorted Vec —
    // BTreeMap iteration is already ascending by hash, satisfying pack_l10n's contract.
    let mut merged: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    for (hash, bytes) in left.collect_entries() {
        merged.insert(hash, bytes);
    }
    for (hash, bytes) in right.collect_entries() {
        merged.insert(hash, bytes);
    }
    let merged: Vec<(u64, Vec<u8>)> = merged.into_iter().collect();
    #[cfg(feature = "debug-keys")]
    let mut debug_keys: BTreeMap<u64, String> = BTreeMap::new();
    #[cfg(feature = "debug-keys")]
    let debug_keys_vec: Vec<(u64, String)> = {
        for (h, k) in left.debug_key_table() {
            debug_keys.insert(h, k);
        }
        for (h, k) in right.debug_key_table() {
            debug_keys.insert(h, k);
        }
        debug_keys.into_iter().collect()
    };
    let locale_data_version = left.locale_data_version().max(right.locale_data_version());
    Ok(pack_l10n(
        &merged,
        RUNTIME_VERSION,
        locale_data_version,
        #[cfg(feature = "debug-keys")]
        if debug_keys_vec.is_empty() {
            None
        } else {
            Some(&debug_keys_vec)
        },
        #[cfg(not(feature = "debug-keys"))]
        None,
    ))
}

/// Packs sorted value blobs into an L10N buffer (v3 when `locale_data_version > 0`, else v2).
///
/// `entries` and `debug_keys` are slices that **must already be sorted ascending by hash**;
/// the binary index is laid out in iteration order. Prefer a `Vec<(u64, Vec<u8>)>` built up
/// and sorted with `sort_unstable_by_key` over a `BTreeMap` — a contiguous buffer is a single
/// allocation versus one per tree node.
#[cfg(feature = "std")]
pub fn pack_l10n(
    entries: &[(u64, Vec<u8>)],
    min_runtime_version: u32,
    locale_data_version: u32,
    #[cfg(feature = "debug-keys")] debug_keys: Option<&[(u64, String)]>,
    #[cfg(not(feature = "debug-keys"))] _debug_keys: Option<&[(u64, String)]>,
) -> Vec<u8> {
    let mut data_pool = Vec::new();
    let mut index_entries = Vec::with_capacity(entries.len());
    let (format_version, header_size) = if locale_data_version > 0 {
        (FORMAT_VERSION_V3, HEADER_SIZE_V3)
    } else {
        (FORMAT_VERSION_V2, HEADER_SIZE_V2)
    };
    let mut current_offset = header_size as u32;

    for &(hash, ref val_bytes) in entries {
        let val_offset = current_offset;
        let val_len = val_bytes.len() as u32;
        data_pool.extend_from_slice(val_bytes);
        current_offset += val_len;
        index_entries.push((hash, val_offset, val_len));
    }

    let index_offset = current_offset;
    let index_count = index_entries.len() as u32;

    for (hash, v_off, v_len) in index_entries {
        data_pool.extend_from_slice(&hash.to_be_bytes());
        data_pool.extend_from_slice(&v_off.to_be_bytes());
        data_pool.extend_from_slice(&v_len.to_be_bytes());
    }

    #[cfg(feature = "debug-keys")]
    if let Some(keys) = debug_keys {
        if !keys.is_empty() {
            data_pool.extend_from_slice(DEBUG_SECTION_MAGIC);
            data_pool.extend_from_slice(&(keys.len() as u32).to_be_bytes());
            for &(hash, ref key) in keys {
                data_pool.extend_from_slice(&hash.to_be_bytes());
                let kb = key.as_bytes();
                data_pool.extend_from_slice(&(kb.len() as u32).to_be_bytes());
                data_pool.extend_from_slice(kb);
            }
        }
    }

    let mut buffer = Vec::with_capacity(header_size + data_pool.len());
    buffer.extend_from_slice(b"L10N");
    buffer.extend_from_slice(&format_version.to_be_bytes());
    buffer.extend_from_slice(&min_runtime_version.to_be_bytes());
    if format_version >= FORMAT_VERSION_V3 {
        buffer.extend_from_slice(&locale_data_version.to_be_bytes());
    }
    buffer.extend_from_slice(&index_offset.to_be_bytes());
    buffer.extend_from_slice(&index_count.to_be_bytes());
    buffer.extend_from_slice(&data_pool);
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn hash(s: &str) -> u64 {
        fnv1a_64(s.as_bytes())
    }

    fn create_minimal_binary() -> Vec<u8> {
        pack_l10n(
            &[],
            RUNTIME_VERSION,
            crate::locale_data::LOCALE_DATA_VERSION,
            None,
        )
    }

    #[test]
    fn new_accepts_valid_v2_header() {
        let data = create_minimal_binary();
        assert!(BinaryFormatReader::new(&data).is_ok());
    }

    #[test]
    fn new_accepts_legacy_v1_header() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&FORMAT_VERSION_V1.to_be_bytes());
        buf.extend_from_slice(&16u32.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
        assert!(BinaryFormatReader::new(&buf).is_ok());
    }

    #[test]
    fn new_rejects_short_buffer() {
        assert!(BinaryFormatReader::new(b"").is_err());
    }

    #[test]
    fn new_rejects_bad_magic() {
        let data =
            b"XXXX\x00\x00\x00\x02\x00\x00\x00\x01\x00\x00\x00\x14\x00\x00\x00\x00\x00\x00\x00";
        assert!(BinaryFormatReader::new(data).is_err());
    }

    #[test]
    fn lookup_roundtrip_v2() {
        let mut entries = BTreeMap::new();
        entries.insert(hash("a"), b"val-a".to_vec());
        let entries: Vec<(u64, Vec<u8>)> = entries.into_iter().collect();
        let buf = pack_l10n(
            &entries,
            RUNTIME_VERSION,
            crate::locale_data::LOCALE_DATA_VERSION,
            None,
        );
        let reader = BinaryFormatReader::new(&buf).unwrap();
        assert_eq!(reader.lookup(hash("a")).unwrap(), b"val-a");
        assert!(reader.lookup(hash("missing")).is_none());
    }

    #[test]
    fn merge_l10n_overrides_on_collision() {
        let mut left = BTreeMap::new();
        left.insert(hash("a"), b"old".to_vec());
        left.insert(hash("b"), b"keep".to_vec());
        let left: Vec<(u64, Vec<u8>)> = left.into_iter().collect();
        let mut right = BTreeMap::new();
        right.insert(hash("a"), b"new".to_vec());
        right.insert(hash("c"), b"added".to_vec());
        let right: Vec<(u64, Vec<u8>)> = right.into_iter().collect();
        let merged = merge_l10n_buffers(
            &pack_l10n(
                &left,
                RUNTIME_VERSION,
                crate::locale_data::LOCALE_DATA_VERSION,
                None,
            ),
            &pack_l10n(
                &right,
                RUNTIME_VERSION,
                crate::locale_data::LOCALE_DATA_VERSION,
                None,
            ),
        )
        .unwrap();
        let reader = BinaryFormatReader::new(&merged).unwrap();
        assert_eq!(reader.lookup(hash("a")).unwrap(), b"new");
        assert_eq!(reader.lookup(hash("b")).unwrap(), b"keep");
        assert_eq!(reader.lookup(hash("c")).unwrap(), b"added");
    }

    #[test]
    fn new_rejects_future_min_runtime() {
        let buf = pack_l10n(
            &[],
            RUNTIME_VERSION + 1,
            crate::locale_data::LOCALE_DATA_VERSION,
            None,
        );
        let err = BinaryFormatReader::new(&buf).unwrap_err();
        assert!(matches!(err, crate::CoreError::RuntimeTooOld { .. }));
    }

    #[test]
    fn new_rejects_unsupported_version() {
        let mut buf = create_minimal_binary();
        buf[4..8].copy_from_slice(&99u32.to_be_bytes());
        assert!(BinaryFormatReader::new(&buf).is_err());
    }

    #[test]
    fn new_rejects_future_locale_data_version() {
        let buf = pack_l10n(
            &[],
            RUNTIME_VERSION,
            crate::locale_data::SUPPORTED_LOCALE_DATA_VERSION + 1,
            None,
        );
        let err = BinaryFormatReader::new(&buf).unwrap_err();
        assert!(matches!(err, crate::CoreError::LocaleDataTooOld { .. }));
    }

    #[test]
    fn v3_header_pins_locale_data_version() {
        let buf = pack_l10n(
            &[],
            RUNTIME_VERSION,
            crate::locale_data::LOCALE_DATA_VERSION,
            None,
        );
        let reader = BinaryFormatReader::new(&buf).unwrap();
        assert_eq!(reader.format_version(), FORMAT_VERSION_V3);
        assert_eq!(
            reader.locale_data_version(),
            crate::locale_data::LOCALE_DATA_VERSION
        );
    }

    #[cfg(feature = "debug-keys")]
    #[test]
    fn debug_key_table_roundtrip() {
        let entries: Vec<(u64, Vec<u8>)> = vec![(hash("common.welcome"), b"hi".to_vec())];
        let keys: Vec<(u64, String)> =
            vec![(hash("common.welcome"), "common.welcome".to_string())];
        let buf = pack_l10n(
            &entries,
            RUNTIME_VERSION,
            crate::locale_data::LOCALE_DATA_VERSION,
            Some(&keys),
        );
        let reader = BinaryFormatReader::new(&buf).unwrap();
        let table = reader.debug_key_table();
        assert_eq!(
            table.get(&hash("common.welcome")).map(String::as_str),
            Some("common.welcome")
        );
    }
}

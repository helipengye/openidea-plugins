use base64::{Engine, engine::general_purpose::STANDARD};
use miniz_oxide::{deflate::compress_to_vec_zlib, inflate::decompress_to_vec_zlib_with_limit};
use openidea_plugin::{Error, host};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{MAX_INDEX_BYTES, Scan, scan::Endpoint};

// Leave room for environments in API 1.2 hosts with a 1 MiB plugin storage quota.
const MAX_CACHE_BYTES: usize = 768 << 10;

pub(super) fn key(root: &str) -> String {
    format!("scan-cache:v1:{root}")
}

pub(super) fn save(root: &str, scan: &Scan) -> Result<u64, Error> {
    let saved_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // Version, time, file count, skipped count, endpoints. Borrow endpoints while serializing.
    let snapshot = serde_json::to_vec(&(1u32, saved_at, scan.files, scan.skipped, &scan.endpoints))
        .map_err(|e| Error::Failed(e.to_string()))?;
    if snapshot.len() > MAX_INDEX_BYTES {
        return Err(Error::Invalid("scan_cache_size".into()));
    }
    let encoded = STANDARD.encode(compress_to_vec_zlib(&snapshot, 1));
    if encoded.len() > MAX_CACHE_BYTES {
        return Err(Error::Invalid("scan_cache_size".into()));
    }
    host::storage::set(&key(root), &encoded)?;
    Ok(saved_at)
}

pub(super) fn load(root: &str) -> Option<Scan> {
    let encoded = host::storage::get(&key(root)).ok()??;
    if encoded.len() > MAX_CACHE_BYTES {
        return None;
    }
    let compressed = STANDARD.decode(encoded).ok()?;
    let snapshot = decompress_to_vec_zlib_with_limit(&compressed, MAX_INDEX_BYTES).ok()?;
    let (version, saved_at, files, skipped, endpoints): (u32, u64, usize, usize, Vec<Endpoint>) =
        serde_json::from_slice(&snapshot).ok()?;
    if version != 1 || skipped > files {
        return None;
    }
    Some(Scan { endpoints, files, skipped, done: true, cached: true, saved_at, ..Scan::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn oversized_cache_does_not_replace_the_previous_snapshot() {
        let host = MockHost::install();
        let mut scan = Scan { done: true, ..Scan::default() };
        save("/project", &scan).unwrap();
        let original = host.storage()[&key("/project")].clone();
        let mut state = 1u32;
        let url: String = (0..1_200_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                char::from(b'!' + (state % 90) as u8)
            })
            .collect();
        scan.endpoints = crate::scan::scan("/project/C.java", "class C { @GetMapping(\"/r\") void m() {} }");
        scan.endpoints[0].url = url;
        assert!(save("/project", &scan).is_err());
        assert_eq!(host.storage()[&key("/project")], original);
    }

    #[test]
    fn unsupported_cache_version_is_ignored() {
        let _host = MockHost::install();
        let snapshot = serde_json::to_vec(&(2u32, 1u64, 0usize, 0usize, Vec::<Endpoint>::new())).unwrap();
        host::storage::set(&key("/project"), &STANDARD.encode(compress_to_vec_zlib(&snapshot, 1))).unwrap();
        assert!(load("/project").is_none());
    }
}

//! Phase 4: content-addressed on-disk store.
//! Layout: <root>/<first 2 hex>/<remaining hex>. Writes are atomic (temp file + rename).
//! Each entry stores a checksum; a corrupt entry is treated as a miss.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::fingerprint::Key;
use crate::Result;

pub struct Cache {
    root: PathBuf,
    limit_bytes: usize,
}

impl Cache {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root, limit_bytes: 64 * 1024 * 1024 })
    }

    pub fn set_limit(&mut self, bytes: usize) {
        self.limit_bytes = bytes;
    }

    fn path_for(&self, key: &Key) -> PathBuf {
        let hex = key.to_hex();
        self.root.join(&hex[..2]).join(&hex[2..])
    }

    /// Returns the artifact bytes on a valid hit; `None` on miss or corruption.
    pub fn get(&self, key: &Key) -> Option<Vec<u8>> {
        let data = fs::read(self.path_for(key)).ok()?;
        if data.len() < 32 {
            return None;
        }
        let (sum, payload) = data.split_at(32);
        (blake3::hash(payload).as_bytes() == sum).then(|| payload.to_vec())
    }

    pub fn put(&self, key: &Key, payload: &[u8]) -> Result<()> {
        let path = self.path_for(key);
        fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(blake3::hash(payload).as_bytes())?;
            f.write_all(payload)?;
        }
        fs::rename(tmp, path)?;
        self.evict_if_needed();
        Ok(())
    }

    fn evict_if_needed(&self) {
        // collect cache files
        let mut files = Vec::new();
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            if let Ok(rd) = fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() { walk(&p, out); }
                    else if p.is_file() { out.push(p); }
                }
            }
        }
        walk(&self.root, &mut files);
        let mut total: u64 = files.iter().filter_map(|p| fs::metadata(p).ok().map(|m| m.len())).sum();
        if total <= self.limit_bytes as u64 { return; }
        // sort by mtime ascending
        files.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
        for p in files {
            if total <= self.limit_bytes as u64 { break; }
            if let Ok(m) = fs::metadata(&p) {
                let len = m.len();
                let _ = fs::remove_file(&p);
                total = total.saturating_sub(len);
            }
        }
    }

    pub fn clear(&self) -> Result<()> {
        if Path::new(&self.root).exists() {
            fs::remove_dir_all(&self.root)?;
        }
        fs::create_dir_all(&self.root)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::KeyBuilder;

    fn tmp_root(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("incc-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn put_get_roundtrip_and_miss() {
        let c = Cache::open(tmp_root("rt")).unwrap();
        let k = KeyBuilder::new("v", "t", "o").finish();
        assert!(c.get(&k).is_none());
        c.put(&k, b"artifact").unwrap();
        assert_eq!(c.get(&k).unwrap(), b"artifact");
    }

    #[test]
    fn corrupt_entry_is_a_miss() {
        let c = Cache::open(tmp_root("corrupt")).unwrap();
        let k = KeyBuilder::new("v", "t", "o").finish();
        c.put(&k, b"artifact").unwrap();
        let path = c.path_for(&k);
        let mut data = fs::read(&path).unwrap();
        *data.last_mut().unwrap() ^= 0xff;
        fs::write(&path, data).unwrap();
        assert!(c.get(&k).is_none());
    }

    #[test]
    fn clear_empties_cache() {
        let c = Cache::open(tmp_root("clear")).unwrap();
        let k = KeyBuilder::new("v", "t", "o").finish();
        c.put(&k, b"x").unwrap();
        c.clear().unwrap();
        assert!(c.get(&k).is_none());
    }
}

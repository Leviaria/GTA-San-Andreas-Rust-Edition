//! FiveM's resource KVP: small key-value storage per resource, kept in
//! `kvp/<resource>.json` next to server.cfg and written at most once a second.
//!
//! Writes are atomic: the new file is written and synced as `.json.tmp`, the
//! previous file becomes `.json.bak`, then the new file is renamed into place.
//! A crash leaves either the old or the new file, never a mix. A file that
//! does not parse is never overwritten: it is moved aside as
//! `.json.corrupt-<time>` and the backup is loaded instead.
use serde_json::Value as Json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_KEY: usize = 255;
pub const MAX_VALUE: usize = 64 * 1024;
/// Total stored bytes (keys and values) per resource.
pub const MAX_RESOURCE: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub struct Store {
    dir: PathBuf,
    data: BTreeMap<String, BTreeMap<String, Json>>,
    dirty: BTreeSet<String>,
    flushed: Option<Instant>,
    /// StartFindKvp handles: matching keys left to return.
    finds: BTreeMap<i64, Vec<String>>,
    next_find: i64,
    /// Recovery messages for the console, taken by `take_warnings`.
    warnings: Vec<String>,
}

fn size(values: &BTreeMap<String, Json>) -> usize {
    values
        .iter()
        .map(|(k, v)| k.len() + v.to_string().len())
        .sum()
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            ..Default::default()
        }
    }
    fn path(&self, resource: &str) -> PathBuf {
        self.dir.join(format!("{resource}.json"))
    }
    fn values(&mut self, resource: &str) -> &mut BTreeMap<String, Json> {
        if !self.data.contains_key(resource) {
            let loaded = load(&self.path(resource), resource, &mut self.warnings);
            self.data.insert(resource.to_string(), loaded);
        }
        self.data.get_mut(resource).unwrap()
    }
    /// Messages about recovered or quarantined files since the last call.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
    pub fn get(&mut self, resource: &str, key: &str) -> Option<Json> {
        self.values(resource).get(key).cloned()
    }
    /// Returns an error message when a limit is exceeded.
    pub fn set(&mut self, resource: &str, key: &str, value: Json) -> Result<(), String> {
        if key.is_empty() || key.len() > MAX_KEY {
            return Err(format!("KVP key must be 1-{MAX_KEY} bytes"));
        }
        if value.to_string().len() > MAX_VALUE {
            return Err(format!("KVP value for {key} is over {MAX_VALUE} bytes"));
        }
        let values = self.values(resource);
        let old = values.insert(key.to_string(), value);
        if size(values) > MAX_RESOURCE {
            match old {
                Some(old) => values.insert(key.to_string(), old),
                None => values.remove(key),
            };
            return Err(format!(
                "KVP storage for {resource} is full ({MAX_RESOURCE} bytes)"
            ));
        }
        self.dirty.insert(resource.to_string());
        Ok(())
    }
    pub fn delete(&mut self, resource: &str, key: &str) {
        if self.values(resource).remove(key).is_some() {
            self.dirty.insert(resource.to_string());
        }
    }
    pub fn start_find(&mut self, resource: &str, prefix: &str) -> i64 {
        let keys: Vec<String> = self
            .values(resource)
            .keys()
            .filter(|k| k.starts_with(prefix))
            .rev()
            .cloned()
            .collect();
        self.next_find += 1;
        // Abandoned handles stay bounded.
        if self.finds.len() >= 64 {
            let oldest = *self.finds.keys().next().unwrap();
            self.finds.remove(&oldest);
        }
        self.finds.insert(self.next_find, keys);
        self.next_find
    }
    pub fn find(&mut self, handle: i64) -> Option<String> {
        self.finds.get_mut(&handle)?.pop()
    }
    pub fn end_find(&mut self, handle: i64) {
        self.finds.remove(&handle);
    }
    /// Write changed resources; `force` ignores the once-a-second limit.
    pub fn flush(&mut self, force: bool) -> Vec<String> {
        if self.dirty.is_empty()
            || (!force
                && self
                    .flushed
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(1)))
        {
            return Vec::new();
        }
        self.flushed = Some(Instant::now());
        let mut errors = Vec::new();
        for resource in std::mem::take(&mut self.dirty) {
            let path = self.path(&resource);
            let data = serde_json::to_vec_pretty(&self.data[&resource]).expect("JSON values");
            if let Err(error) = write_atomic(&path, &data) {
                errors.push(format!("Cannot save KVP for {resource}: {error}"));
                self.dirty.insert(resource);
            }
        }
        errors
    }
}

type Values = BTreeMap<String, Json>;

fn parse(path: &Path) -> Option<std::io::Result<Values>> {
    match fs::read(path) {
        Ok(bytes) => Some(
            serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(e)),
    }
}

/// Read a resource's file, falling back to its backup. A broken file is
/// moved aside so that the next save cannot overwrite it.
fn load(path: &Path, resource: &str, warnings: &mut Vec<String>) -> Values {
    let temporary = path.with_extension("json.tmp");
    if temporary.exists() {
        // An interrupted save; the file it was replacing is still intact.
        let _ = fs::remove_file(&temporary);
        warnings.push(format!(
            "KVP for {resource}: removed an unfinished save ({}).",
            temporary.display()
        ));
    }
    let backup = path.with_extension("json.bak");
    let problem = match parse(path) {
        Some(Ok(values)) => return values,
        None => None,
        Some(Err(error)) => {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let aside = path.with_extension(format!("json.corrupt-{stamp}"));
            let moved = fs::rename(path, &aside);
            Some(match moved {
                Ok(()) => format!("{error}; kept as {}", aside.display()),
                Err(move_error) => format!("{error}; could not move it aside: {move_error}"),
            })
        }
    };
    match (problem, parse(&backup)) {
        (None, None) => Values::new(),
        (None, Some(Ok(values))) => {
            warnings.push(format!(
                "KVP for {resource}: {} was missing (interrupted save); restored the backup.",
                path.display()
            ));
            values
        }
        (Some(problem), Some(Ok(values))) => {
            warnings.push(format!(
                "KVP for {resource}: {} is unreadable ({problem}); loaded the backup from the previous save.",
                path.display()
            ));
            values
        }
        (problem, backup_state) => {
            warnings.push(format!(
                "KVP for {resource}: no readable save ({}{}); starting empty.",
                problem.unwrap_or_else(|| "file missing".into()),
                match backup_state {
                    Some(Err(e)) => format!(", backup unreadable: {e}"),
                    _ => String::new(),
                }
            ));
            Values::new()
        }
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir)?;
    let temporary = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    if path.exists() {
        fs::rename(path, path.with_extension("json.bak"))?;
    }
    fs::rename(&temporary, path)?;
    // Make the renames durable (directories cannot be opened on Windows).
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_persist_limits_hold_and_finds_iterate_by_prefix() {
        let dir = std::env::temp_dir().join(format!("sare-kvp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut store = Store::new(dir.clone());
        store
            .set("bank", "account:1", json!("{\"cash\":5}"))
            .unwrap();
        store.set("bank", "account:2", json!(7)).unwrap();
        store.set("bank", "other", json!(1.5)).unwrap();
        assert!(store.set("bank", "", json!(1)).is_err());
        assert!(store
            .set("bank", "big", json!("x".repeat(MAX_VALUE + 1)))
            .is_err());
        let handle = store.start_find("bank", "account:");
        assert_eq!(store.find(handle).as_deref(), Some("account:1"));
        assert_eq!(store.find(handle).as_deref(), Some("account:2"));
        assert_eq!(store.find(handle), None);
        store.end_find(handle);
        store.delete("bank", "other");
        assert!(store.flush(false).is_empty());
        let mut reloaded = Store::new(dir.clone());
        assert_eq!(reloaded.get("bank", "account:2"), Some(json!(7)));
        assert_eq!(reloaded.get("bank", "other"), None);
        let _ = fs::remove_dir_all(dir);
    }

    fn saved(dir: &Path, value: i64) {
        let mut store = Store::new(dir.to_path_buf());
        store.set("core", "money", json!(value)).unwrap();
        assert!(store.flush(true).is_empty());
    }

    #[test]
    fn corrupt_or_interrupted_saves_never_replace_good_data() {
        let dir = std::env::temp_dir().join(format!("sare-kvp-recover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let file = dir.join("core.json");
        saved(&dir, 1);
        saved(&dir, 2);
        // The previous save is kept as a backup.
        assert!(fs::read_to_string(dir.join("core.json.bak"))
            .unwrap()
            .contains('1'));

        // A crash while writing leaves a partial temporary file: ignored.
        fs::write(dir.join("core.json.tmp"), "{\"money\": 9").unwrap();
        let mut store = Store::new(dir.clone());
        assert_eq!(store.get("core", "money"), Some(json!(2)));
        assert!(!dir.join("core.json.tmp").exists());
        assert_eq!(store.take_warnings().len(), 1);

        // A crash between the two renames: the backup is used.
        fs::rename(&file, dir.join("core.json.bak")).unwrap();
        let mut store = Store::new(dir.clone());
        assert_eq!(store.get("core", "money"), Some(json!(2)));
        assert!(store.take_warnings()[0].contains("restored the backup"));
        assert!(store.flush(true).is_empty()); // nothing changed, nothing written
        saved(&dir, 3);

        // A damaged file is moved aside, the backup loads, and saving again
        // does not destroy the damaged copy.
        fs::write(&file, "{\"money\": 3,,,").unwrap();
        let mut store = Store::new(dir.clone());
        assert_eq!(store.get("core", "money"), Some(json!(2)));
        assert!(store.take_warnings()[0].contains("unreadable"));
        store.set("core", "money", json!(4)).unwrap();
        assert!(store.flush(true).is_empty());
        let corrupt: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt"))
            .collect();
        assert_eq!(corrupt.len(), 1);
        assert!(fs::read_to_string(corrupt[0].path())
            .unwrap()
            .contains(",,,"));
        assert_eq!(Store::new(dir.clone()).get("core", "money"), Some(json!(4)));

        // Both unreadable: start empty but keep the broken file.
        fs::write(&file, "x").unwrap();
        fs::write(dir.join("core.json.bak"), "y").unwrap();
        let mut store = Store::new(dir.clone());
        assert_eq!(store.get("core", "money"), None);
        assert!(store.take_warnings()[0].contains("starting empty"));
        let _ = fs::remove_dir_all(dir);
    }
}

//! Where the index lives, and how it is written without ever being torn.

use std::io::Write;
use std::path::{Path, PathBuf};

use pl_index::codec::{self, Library, OpenError};

/// The OS cache directory for indexes, creating it if needed.
///
/// **Not beside the data.** The motivating scenario is a shared drive, and a
/// sync client does not implement rename-over-inode across machines: writing a
/// multi-megabyte index into a synced folder means every lab member replicates
/// it on every rescan, and two people scanning at once produce
/// `library-LIOR-PC.plx` conflict copies that nothing ever cleans up.
///
/// ```text
/// Windows   %LOCALAPPDATA%\Polylinker\index
/// macOS     ~/Library/Caches/Polylinker/index
/// other     $XDG_CACHE_HOME/polylinker/index, else ~/.cache/polylinker/index
/// ```
pub fn cache_dir() -> Result<PathBuf, String> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is not set")?
            .join("Polylinker")
    } else if cfg!(target_os = "macos") {
        home()?.join("Library/Caches/Polylinker")
    } else {
        match std::env::var_os("XDG_CACHE_HOME") {
            Some(v) => PathBuf::from(v).join("polylinker"),
            None => home()?.join(".cache/polylinker"),
        }
    };
    let dir = base.join("index");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

/// The index file for a root: a readable slug plus a hash of the full path.
///
/// The slug is for the human reading the directory; the hash is what makes it
/// unique, since `plasmids` under two different parents is two libraries.
pub fn index_path(dir: &Path, root: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let key = canonical.to_string_lossy().to_lowercase();
    let hash = pl_core::sha1::sha1(key.as_bytes());
    let short: String = hash[..4].iter().map(|b| format!("{b:02x}")).collect();
    let slug: String = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "root".into())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(40)
        .collect();
    dir.join(format!("{slug}-{short}.plx"))
}

#[derive(Debug)]
pub enum SaveError {
    Io(String),
    /// The live file was written by a newer build. Refused rather than
    /// overwritten: with a shared index that would destroy a colleague's work.
    WouldClobberNewer(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "{e}"),
            SaveError::WouldClobberNewer(e) => write!(f, "{e}"),
        }
    }
}

/// Write an index, atomically.
///
/// Temp file → `flush` → `sync_all` → `rename` over the live path. The live
/// file is **never opened for writing**, so no window exists in which a reader
/// could see it torn; a crash leaves an orphaned temporary and an intact index.
///
/// The temporary carries the process id and a counter rather than a fixed name.
/// `File::create` on Windows opens with `FILE_SHARE_WRITE`, so two concurrent
/// scans sharing one `*.new` would interleave into a single file and both
/// report success. With unique names both rename, the last one wins, and both
/// files were complete.
///
/// No lock file, and therefore no stale-lock recovery problem.
pub fn save(path: &Path, lib: &Library) -> Result<(), SaveError> {
    // Refuse to overwrite something newer than this build can read.
    if let Ok(existing) = std::fs::read(path) {
        if let Err(e @ OpenError::FromTheFuture { .. }) = codec::parse(&existing) {
            return Err(SaveError::WouldClobberNewer(format!(
                "{}: {e}",
                path.display()
            )));
        }
    }

    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| SaveError::Io(format!("{}: {e}", dir.display())))?;

    let stem = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "index.plx".into());
    let tmp = dir.join(format!("{stem}.{}.{}.tmp", std::process::id(), next_seq()));

    let bytes = codec::to_bytes(lib);
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.flush()?;
        // Durability before visibility: a rename that lands before the data
        // does gives a file that is present and empty.
        f.sync_all()?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(SaveError::Io(format!("{}: {e}", tmp.display())));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        // The likeliest failure on a real Windows machine: antivirus or a sync
        // client holding a handle. Say which file and what the OS said, and
        // that nothing was lost.
        return Err(SaveError::Io(format!(
            "{}: {e} — the previous index is intact",
            path.display()
        )));
    }
    sweep_stale_temps(dir, &stem);
    Ok(())
}

/// A per-process counter, so two saves in the same millisecond differ.
fn next_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Remove abandoned temporaries from crashed runs.
///
/// Best-effort and silent about individual failures: another process may hold
/// one open, and failing a scan over a stale temp would be absurd.
fn sweep_stale_temps(dir: &Path, stem: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let cutoff = std::time::Duration::from_secs(3600);
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with(stem) || !name.ends_with(".tmp") {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|d| d > cutoff).unwrap_or(false))
            .unwrap_or(false);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Read an index. `Ok(None)` means there is no index yet, which is not an error.
pub fn load(path: &Path) -> Result<Option<Library>, OpenError> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(OpenError::BadTable(format!("{}: {e}", path.display())));
        }
    };
    codec::parse(&data).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_index::{Row, State};

    fn tmpdir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("pl-scan-store-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample() -> Library {
        Library {
            root: "C:/lab".into(),
            built_ns: 7,
            complete: true,
            rows: vec![Row {
                path: "a.gb".into(),
                state: State::NoBases,
                name: "a".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn an_index_survives_a_write_and_a_read() {
        let dir = tmpdir("roundtrip");
        let path = dir.join("lib.plx");
        assert!(
            load(&path).unwrap().is_none(),
            "no index yet is not an error"
        );
        save(&path, &sample()).unwrap();
        assert_eq!(load(&path).unwrap().unwrap(), sample());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_leaves_no_temporary_behind() {
        let dir = tmpdir("notemp");
        let path = dir.join("lib.plx");
        save(&path, &sample()).unwrap();
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reader_holding_the_old_file_open_does_not_block_the_write() {
        // The Windows question: rename over a path that someone has open.
        let dir = tmpdir("openreader");
        let path = dir.join("lib.plx");
        save(&path, &sample()).unwrap();
        let handle = std::fs::File::open(&path).unwrap();

        let mut next = sample();
        next.rows[0].name = "renamed".into();
        save(&path, &next).expect("rename over an open reader must succeed");
        drop(handle);
        assert_eq!(load(&path).unwrap().unwrap().rows[0].name, "renamed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_index_is_never_overwritten() {
        // With a shared index this would destroy work this build cannot
        // reproduce, so it is refused at the write, not only at the read.
        let dir = tmpdir("future");
        let path = dir.join("lib.plx");
        let mut bytes = pl_index::codec::to_bytes(&sample());
        bytes[8..12].copy_from_slice(&(pl_index::FORMAT + 1).to_be_bytes());
        let n = bytes.len();
        let digest = pl_core::sha1::sha1(&bytes[..n - 20]);
        bytes[n - 20..].copy_from_slice(&digest);
        std::fs::write(&path, &bytes).unwrap();

        let err = save(&path, &sample()).unwrap_err();
        assert!(matches!(err, SaveError::WouldClobberNewer(_)), "{err}");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "the file is untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_index_is_reported_rather_than_half_read() {
        let dir = tmpdir("damaged");
        let path = dir.join("lib.plx");
        save(&path, &sample()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let n = bytes.len();
        bytes[n / 2] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();

        let err = load(&path).unwrap_err();
        assert!(
            err.rebuildable(),
            "a damaged cache is a rebuild, not a stop"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_roots_with_the_same_basename_get_different_index_files() {
        // `plasmids` under two parents is two libraries, and one file for both
        // would silently answer about the wrong folder.
        let dir = PathBuf::from("/cache");
        let a = index_path(&dir, Path::new("/lab/alice/plasmids"));
        let b = index_path(&dir, Path::new("/lab/bob/plasmids"));
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("plasmids"), "{a:?}");
        assert!(a.extension().unwrap() == "plx");
    }

    #[test]
    fn an_index_name_is_safe_whatever_the_folder_is_called() {
        let dir = PathBuf::from("/cache");
        let p = index_path(&dir, Path::new("/lab/my plasmids (2024)/"));
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'),
            "{name}"
        );
    }

    #[test]
    fn concurrent_saves_both_produce_a_complete_file() {
        // Unique temp names are the point: a fixed `*.new` on Windows opens
        // with FILE_SHARE_WRITE, so two writers interleave into one file and
        // both report success.
        let dir = tmpdir("concurrent");
        let path = dir.join("lib.plx");
        std::thread::scope(|s| {
            for i in 0..8 {
                let path = path.clone();
                s.spawn(move || {
                    let mut lib = sample();
                    lib.rows[0].name = format!("writer {i}");
                    save(&path, &lib).unwrap();
                });
            }
        });
        // Whoever won, the file parses and holds one complete row.
        let lib = load(&path).unwrap().unwrap();
        assert_eq!(lib.rows.len(), 1);
        assert!(
            lib.rows[0].name.starts_with("writer "),
            "{:?}",
            lib.rows[0].name
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Enumerating a folder, on a real lab drive.

use std::path::{Path, PathBuf};

/// Extensions worth opening. Format is still decided from **content** — this
/// only decides what to read at all, because opening 68,813 files to sniff four
/// bytes each is a minute of I/O for nothing.
pub const EXTENSIONS: &[&str] = &[
    "dna", "gb", "gbk", "genbank", "gbff", "fa", "fasta", "fna", "ffn", "seq", "ape", "ab1", "scf",
    "ztr",
];

#[derive(Debug, Clone)]
pub struct WalkOptions {
    /// Follow symbolic links. Off by default, because a link back to an
    /// ancestor makes the walk unbounded.
    ///
    /// This is about *symlinks*, not reparse points. On the measured corpus
    /// 68,811 of 68,813 files are reparse points — OneDrive tags every file it
    /// manages — so a walker that skipped reparse points would skip the entire
    /// library and report nothing, with no error.
    pub follow_links: bool,
    /// A hard bound, so a link cycle or a pathological tree terminates.
    pub max_depth: usize,
    /// Directory names to skip entirely, matched exactly.
    pub skip_dirs: Vec<String>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        WalkOptions {
            follow_links: false,
            max_depth: 32,
            skip_dirs: vec![
                ".git".into(),
                "node_modules".into(),
                "target".into(),
                ".polylinker".into(),
            ],
        }
    }
}

/// One candidate file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    /// Relative to the root, `/`-separated.
    pub rel: String,
    pub size: u64,
    /// Nanoseconds since the Unix epoch, or 0 if the platform declined to say.
    pub mtime_ns: i128,
    /// A cloud placeholder that is not materialised locally.
    ///
    /// Windows only; there is no portable equivalent, and pretending otherwise
    /// would be worse than saying so. On the measured corpus this is **always
    /// false** — every file is `PINNED` — but a colleague with Files On-Demand
    /// enabled would see otherwise.
    pub offline: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WalkReport {
    pub dirs: usize,
    pub files_considered: usize,
    /// Directories that could not be listed, with the reason.
    pub errors: Vec<(String, String)>,
    /// Set when the walk did not finish. **The caller must not remove any
    /// rows** when this is set: a partial walk read as a mass deletion is how
    /// a library empties itself because a share blinked.
    pub incomplete: Option<String>,
    pub placeholders: usize,
}

/// Enumerate candidate sequence files under `root`.
///
/// Breadth-first with an explicit stack, so a deep tree cannot overflow it, and
/// depth-bounded so a symlink cycle terminates even with `follow_links`.
///
/// Metadata comes from the directory entry, never a second `stat`: on Windows
/// `FindNextFileW` has already returned size and mtime, and the measured cost
/// of enumerate-only over 68,813 files is 2.1 s warm against 15.2 s for the
/// filtered form that re-queries.
pub fn walk(root: &Path, opts: &WalkOptions) -> (Vec<Found>, WalkReport) {
    let mut out = Vec::new();
    let mut report = WalkReport::default();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if depth > opts.max_depth {
            report.errors.push((
                dir.display().to_string(),
                format!("deeper than --max-depth {}", opts.max_depth),
            ));
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                report
                    .errors
                    .push((dir.display().to_string(), e.to_string()));
                // A root that cannot be listed at all is not a partial answer,
                // it is no answer; the caller must not treat it as "everything
                // was deleted".
                if dir == root {
                    report.incomplete = Some(format!("{}: {e}", dir.display()));
                }
                continue;
            }
        };
        report.dirs += 1;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    report
                        .errors
                        .push((dir.display().to_string(), e.to_string()));
                    report.incomplete = Some(format!("{}: {e}", dir.display()));
                    continue;
                }
            };
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                report
                    .errors
                    .push((path.display().to_string(), "metadata unavailable".into()));
                continue;
            };

            if meta.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if opts.skip_dirs.contains(&name) {
                    continue;
                }
                if meta.is_symlink() && !opts.follow_links {
                    continue;
                }
                stack.push((path, depth + 1));
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            report.files_considered += 1;

            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let offline = is_placeholder(&meta);
            if offline {
                report.placeholders += 1;
            }
            out.push(Found {
                rel: crate::rel(root, &path),
                path,
                size: meta.len(),
                mtime_ns: mtime_ns(&meta),
                offline,
            });
        }
    }

    // A total order, so a rescan and a rebuild agree byte for byte. Directory
    // iteration order is not guaranteed by any filesystem.
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    (out, report)
}

fn mtime_ns(meta: &std::fs::Metadata) -> i128 {
    meta.modified()
        .ok()
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_nanos() as i128)
        })
        .unwrap_or(0)
}

/// Is this a cloud file that is not present locally?
#[cfg(windows)]
fn is_placeholder(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x0000_1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    let a = meta.file_attributes();
    // Deliberately *not* testing FILE_ATTRIBUTE_REPARSE_POINT. On the measured
    // corpus 68,811 of 68,813 files carry it — OneDrive tags every file it
    // manages, materialised or not — so treating it as "not downloaded" would
    // report an entire library as unavailable.
    a & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(not(windows))]
fn is_placeholder(_meta: &std::fs::Metadata) -> bool {
    // No portable equivalent on macOS or Linux. Saying so beats guessing.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("pl-scan-walk-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = crate::abs(root, rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn only_candidate_extensions_are_returned_and_in_a_total_order() {
        let root = tmp("exts");
        write(&root, "b.gb", "x");
        write(&root, "a.dna", "x");
        write(&root, "notes.txt", "x");
        write(&root, "sheet.xlsx", "x");
        write(&root, "sub/c.fasta", "x");
        let (found, report) = walk(&root, &WalkOptions::default());
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["a.dna", "b.gb", "sub/c.fasta"]);
        assert!(report.incomplete.is_none());
        assert_eq!(report.files_considered, 5, "everything was looked at once");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_order_is_stable_across_repeated_walks() {
        // Byte-equality of rescan against rebuild depends on it, and no
        // filesystem guarantees directory iteration order.
        let root = tmp("order");
        for i in 0..40 {
            write(&root, &format!("d{}/f{i}.gb", i % 4), "x");
        }
        let first = walk(&root, &WalkOptions::default()).0;
        for _ in 0..5 {
            assert_eq!(walk(&root, &WalkOptions::default()).0, first);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_skipped_directory_is_not_descended() {
        let root = tmp("skip");
        write(&root, "keep.gb", "x");
        write(&root, ".git/objects/x.gb", "x");
        write(&root, "node_modules/pkg/y.gb", "x");
        let (found, _) = walk(&root, &WalkOptions::default());
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["keep.gb"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn depth_is_bounded_and_the_truncation_is_reported() {
        let root = tmp("depth");
        write(&root, "a/b/c/d/deep.gb", "x");
        write(&root, "shallow.gb", "x");
        let opts = WalkOptions {
            max_depth: 2,
            ..Default::default()
        };
        let (found, report) = walk(&root, &opts);
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["shallow.gb"]);
        assert!(
            report.errors.iter().any(|(_, e)| e.contains("max-depth")),
            "a truncated walk must say so: {:?}",
            report.errors
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_root_that_cannot_be_listed_is_incomplete_not_empty() {
        // The distinction that stops a library emptying itself when a share
        // blinks: "I found nothing" and "I could not look" are different.
        let mut missing = std::env::temp_dir();
        missing.push("pl-scan-walk-definitely-not-here");
        let _ = std::fs::remove_dir_all(&missing);
        let (found, report) = walk(&missing, &WalkOptions::default());
        assert!(found.is_empty());
        assert!(
            report.incomplete.is_some(),
            "an unreadable root must never look like an empty one"
        );
    }

    #[test]
    fn size_and_mtime_come_back_with_each_file() {
        let root = tmp("meta");
        write(&root, "x.gb", "hello");
        let (found, _) = walk(&root, &WalkOptions::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, 5);
        assert!(
            found[0].mtime_ns > 0,
            "a real mtime is needed for the ledger"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_extension_is_matched_case_insensitively() {
        let root = tmp("case");
        write(&root, "A.GB", "x");
        write(&root, "b.Dna", "x");
        let (found, _) = walk(&root, &WalkOptions::default());
        assert_eq!(found.len(), 2, "a lab drive is full of .GB and .DNA");
        let _ = std::fs::remove_dir_all(&root);
    }
}

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
    ///
    /// When it is on, `max_depth` is the only thing standing between a link
    /// cycle and an endless walk — and hitting that bound marks the walk
    /// incomplete, so a cycle costs a truncated index that says it is
    /// truncated, never a silently emptied one.
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
    ///
    /// Set by every path that did not look at something it was asked to look
    /// at: a directory that could not be listed — the root or any other — a
    /// depth bound that cut a sub-tree off, an entry the directory iterator
    /// failed on, an entry we could not stat, and a link we were asked to
    /// follow whose target would not resolve.
    pub incomplete: Option<String>,
    pub placeholders: usize,
    /// Symbolic links and Windows junctions that were not followed because
    /// `follow_links` is off.
    ///
    /// Not an error: it is the default and it is what keeps a link back to an
    /// ancestor from making the walk unbounded. It is counted rather than
    /// invisible because the entry is skipped whole, and on the other side of
    /// one of these there may be a folder of constructs.
    pub links_skipped: usize,
}

/// Enumerate candidate sequence files under `root`.
///
/// Depth-first with an explicit stack — `Vec::pop` is LIFO — so a deep tree
/// cannot overflow the call stack, and depth-bounded so a symlink cycle
/// terminates even with `follow_links`.
///
/// This line said "breadth-first" while the loop did the opposite, which is the
/// wrong thing to hand the next person reasoning about the peak size of `stack`
/// or about the order `errors` and `incomplete` are filled in. Depth-first is
/// also the order to keep, so do not correct it the other way: `pop_front` on a
/// `VecDeque` would genuinely be breadth-first and would hold a whole level of
/// the tree at once — 65,536 pending directories at branching 4 and depth 8,
/// against 25 for this loop.
///
/// Metadata comes from the directory entry, never a second `stat`: on Windows
/// `FindNextFileW` has already returned size and mtime, and the measured cost
/// of enumerate-only over 68,813 files is 2.1 s warm against 15.2 s for the
/// filtered form that re-queries.
pub fn walk(root: &Path, opts: &WalkOptions) -> (Vec<Found>, WalkReport) {
    let mut out = Vec::new();
    let mut report = WalkReport::default();
    // Each directory carries the canonical identities of the directories on the
    // descent path that reached it. Under --follow-links this breaks symlink
    // cycles: a link whose target canonicalizes to a directory already on the
    // path — a `link -> .`, a link to an ancestor, or a pair of links pointing
    // through each other — is skipped rather than re-entered, which `max_depth`
    // alone only bounds after re-walking the whole sub-tree ~32× under new `rel`
    // paths. A link to a *sibling* is not on the path, so it is still followed
    // and indexed under its own name. The canonicalize cost (one stat per
    // directory) is paid only when links are followed; a plain walk cannot cycle
    // and carries no chain.
    let root_chain = if opts.follow_links {
        std::fs::canonicalize(root)
            .map(|c| vec![c])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut stack: Vec<(PathBuf, usize, Vec<PathBuf>)> = vec![(root.to_path_buf(), 0, root_chain)];

    while let Some((dir, depth, chain)) = stack.pop() {
        if depth > opts.max_depth {
            let why = format!("deeper than --max-depth {}", opts.max_depth);
            report.errors.push((dir.display().to_string(), why.clone()));
            // Truncation is a partial walk, and a partial walk must never read
            // as a deletion. `--max-depth 0` over `root/{a.gb, sub/b.gb}` used
            // to return one file with `incomplete = None`, so the caller
            // dropped `sub/b.gb` from the index, wrote `complete: 1`, and every
            // later `pl find` answered "not in my library" for a plasmid still
            // sitting on disk.
            report.incomplete = Some(format!("{}: {why}", dir.display()));
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                report
                    .errors
                    .push((dir.display().to_string(), e.to_string()));
                // Any directory, not only the root. A sub-tree that cannot be
                // listed -- an ACL change, a network share that dropped -- is
                // no answer about *that sub-tree*, and the caller must not
                // treat it as "everything under it was deleted". The cost of
                // the conservative reading is that a permanently unreadable
                // sub-directory keeps deletions from ever being recorded; the
                // cost of the other reading is a library that empties itself.
                report.incomplete = Some(format!("{}: {e}", dir.display()));
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
            let link_meta = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    // An entry we were handed and could not stat is a thing we
                    // were asked to look at and did not, so it must read as a
                    // partial walk and never as a deletion — the same rule the
                    // `read_dir` arm above follows. This is infallible on
                    // Windows, where the directory iterator has already
                    // returned the attributes, but live on Unix: a directory
                    // that is readable but not searchable (mode 0600) lists
                    // every child and fails `lstat` on all of them, and with
                    // `incomplete` left unset every row under it was dropped
                    // and the index stamped `complete: 1` — no `--follow-links`
                    // needed. The error is now carried through instead of the
                    // literal "metadata unavailable", because EACCES and ENOENT
                    // call for different actions and the operator was being
                    // handed neither.
                    report
                        .errors
                        .push((path.display().to_string(), e.to_string()));
                    report.incomplete = Some(format!("{}: {e}", path.display()));
                    continue;
                }
            };

            // `DirEntry::metadata` does not traverse links on **any** platform,
            // and `FileType`'s three predicates are mutually exclusive — so a
            // symlink, or a Windows junction, pointing at a directory answers
            // `false` to `is_dir()` *and* `false` to `is_file()`. It used to
            // fall through to the `!is_file()` skip below and disappear with no
            // error and no count, which made `--follow-links` a complete no-op
            // on every platform and left the `is_dir() && is_symlink()` test
            // that was meant to guard it unreachable. A link to a directory of
            // 400 `.gb` files contributed nothing while the index still claimed
            // to be complete.
            //
            // Resolving the target costs a second `stat`, but only for the
            // entries that really are links: OneDrive tags 68,811 of the
            // corpus's 68,813 files as reparse points, and none of those carry
            // the name-surrogate bit that `is_symlink()` tests, so the
            // enumerate-once cost of a real lab drive is unchanged.
            let meta = if link_meta.is_symlink() {
                if !opts.follow_links {
                    // Documented behaviour rather than a failure — a link back
                    // to an ancestor makes the walk unbounded — but counted, so
                    // an absent sub-tree is at least a number the caller has.
                    report.links_skipped += 1;
                    continue;
                }
                match std::fs::metadata(&path) {
                    Ok(m) => m,
                    Err(e) => {
                        // A link we were asked to follow and could not resolve
                        // is a sub-tree we did not look at, so the walk is
                        // partial. Recording it only in `errors` meant a
                        // junction to a volume that went away — a dropped drive
                        // mapping, an unplugged disk, a share that blinked —
                        // read as a mass deletion: measured, `2 removed` and
                        // `complete: 1` written for two plasmids still sitting
                        // on disk, byte-identical, so `pl library` had nothing
                        // to warn about and `pl find` answered "not in my
                        // library" for both.
                        //
                        // Deliberately not carved out by `ErrorKind::NotFound`:
                        // that volume case is ERROR_PATH_NOT_FOUND, which Rust
                        // maps to `NotFound` exactly like a genuinely dangling
                        // link (as it does ERROR_BAD_NETPATH), so the kind
                        // cannot tell "the files are gone" from "the road to
                        // them is". The cost is the one the `read_dir` arm
                        // above already weighs and accepts: a permanently
                        // unresolvable link keeps deletions from being
                        // recorded, which beats a library that empties itself.
                        report
                            .errors
                            .push((path.display().to_string(), e.to_string()));
                        report.incomplete = Some(format!("{}: {e}", path.display()));
                        continue;
                    }
                }
            } else {
                link_meta
            };

            if meta.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if opts.skip_dirs.contains(&name) {
                    continue;
                }
                if opts.follow_links {
                    // Enter each real directory once per descent path. A link
                    // whose target is already on this path is a cycle and is
                    // skipped — complete, not partial, since the target's files
                    // are reached by their real path — while a link to a sibling
                    // is not on the path and is followed and indexed under its own
                    // name.
                    match std::fs::canonicalize(&path) {
                        Ok(real) => {
                            if chain.contains(&real) {
                                report.links_skipped += 1;
                                continue;
                            }
                            let mut child = chain.clone();
                            child.push(real);
                            stack.push((path, depth + 1, child));
                        }
                        Err(e) => {
                            report
                                .errors
                                .push((path.display().to_string(), e.to_string()));
                            report.incomplete = Some(format!("{}: {e}", path.display()));
                        }
                    }
                } else {
                    stack.push((path, depth + 1, Vec::new()));
                }
                continue;
            }
            if !meta.is_file() {
                // Neither a file, nor a directory, nor a link: a device node, a
                // FIFO, a socket. There is nothing here to parse.
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
    fn sub_directories_are_visited_last_in_first_out_as_the_doc_now_says() {
        // The doc on `walk` said "breadth-first" for as long as the file has
        // existed while `Vec::pop` did the opposite. Nothing could catch that,
        // because `out` is sorted before it is returned and no other test looks
        // at anything order-sensitive — so the word is pinned here instead.
        //
        // It also guards the direction of the fix: correcting the *loop* to
        // match the old word — `VecDeque::pop_front` — would hold a whole level
        // of the tree pending at once, 65,536 directories at branching 4 and
        // depth 8 against 25 for this loop.
        let root = tmp("lifo");
        for i in 0..3 {
            write(&root, &format!("s{i}/child/f.gb"), "x");
        }
        // Whatever order this filesystem hands the three sub-directories back
        // in — no filesystem promises one — is the order they are pushed in.
        let mut pushed: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(pushed.len(), 3);

        // `--max-depth 1` cuts every `child` off, and each cut appends one
        // `errors` entry as it is popped. That is the only externally visible
        // trace of the traversal order.
        let opts = WalkOptions {
            max_depth: 1,
            ..Default::default()
        };
        let (_, report) = walk(&root, &opts);
        let visited: Vec<String> = report
            .errors
            .iter()
            .map(|(p, _)| {
                Path::new(p)
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        pushed.reverse();
        assert_eq!(
            visited, pushed,
            "the last directory pushed must be the first one visited"
        );
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
    fn a_depth_truncated_walk_is_incomplete_and_not_merely_noted() {
        // The `errors` entry is transient: it goes to stderr once. `incomplete`
        // is what the caller reads to decide whether the files it did not see
        // were deleted, and what gets written into the index as `complete: 0`.
        // With it left unset, `--max-depth 0` over `root/{a.gb, sub/b.gb}`
        // dropped `sub/b.gb` and stamped the index complete.
        let root = tmp("depth-incomplete");
        write(&root, "a.gb", "x");
        write(&root, "sub/b.gb", "x");
        let opts = WalkOptions {
            max_depth: 0,
            ..Default::default()
        };
        let (found, report) = walk(&root, &opts);
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["a.gb"], "the depth bound did cut the sub-tree");
        assert!(
            report.incomplete.is_some(),
            "a walk that did not descend has not finished: {:?}",
            report.errors
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_walk_that_reached_everything_is_not_marked_incomplete() {
        // The control for the two `incomplete` paths above: over-reporting it
        // would make every scan refuse to record a deletion, which is the same
        // library-never-converges failure from the other side.
        let root = tmp("complete");
        write(&root, "a.gb", "x");
        write(&root, "sub/deep/b.gb", "x");
        let (found, report) = walk(&root, &WalkOptions::default());
        assert_eq!(found.len(), 2);
        assert!(report.incomplete.is_none(), "{:?}", report.incomplete);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Make `link` a link to the directory `target`.
    ///
    /// A junction on Windows rather than a symbolic link: `mklink /D` needs
    /// elevation or Developer Mode, `mklink /J` needs neither, and Rust reports
    /// both as `is_symlink()` because both carry the name-surrogate bit.
    #[cfg(windows)]
    fn link_dir(target: &Path, link: &Path) {
        let out = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.display().to_string(),
                &target.display().to_string(),
            ])
            .output()
            .expect("mklink");
        assert!(
            out.status.success(),
            "could not create a junction: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[cfg(not(windows))]
    fn link_dir(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).expect("symlink");
    }

    #[test]
    fn a_link_to_a_directory_is_walked_when_following_is_asked_for() {
        // `DirEntry::metadata` never traverses a link, and `is_dir`/`is_file`/
        // `is_symlink` are mutually exclusive, so a link to a directory answers
        // false to both of the first two. It fell through the `!is_file()` skip
        // and was dropped with no error, which made `--follow-links` a no-op
        // and silently lost the linked sub-tree from the index.
        let root = tmp("followlinks");
        write(&root, "plain.gb", "x");
        write(&root, "realdir/a.gb", "x");
        link_dir(&crate::abs(&root, "realdir"), &crate::abs(&root, "linkdir"));

        let opts = WalkOptions {
            follow_links: true,
            ..Default::default()
        };
        let (found, report) = walk(&root, &opts);
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(
            rels,
            vec!["linkdir/a.gb", "plain.gb", "realdir/a.gb"],
            "the file behind the link is indexed under the path the user gave"
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.links_skipped, 0, "nothing was skipped");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_symlink_cycle_is_entered_once_not_re_walked_to_max_depth() {
        // `link -> .` (or a link to an ancestor) makes `--follow-links` unbounded
        // if `max_depth` is the only guard: the tree is re-walked ~32× under new
        // `rel` paths. The per-descent-path identity check enters the cycle once
        // and skips it, so a file is not multiplied and the walk stays complete.
        let root = tmp("cycle");
        write(&root, "a.gb", "x");
        link_dir(&crate::abs(&root, "."), &crate::abs(&root, "loop"));
        let opts = WalkOptions {
            follow_links: true,
            ..Default::default()
        };
        let (found, report) = walk(&root, &opts);
        let a = found.iter().filter(|f| f.rel.ends_with("a.gb")).count();
        assert_eq!(
            a,
            1,
            "the self-link re-walked the tree: {} rows",
            found.len()
        );
        assert!(report.links_skipped >= 1, "the cycle was not skipped");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_link_is_not_followed_by_default_and_the_skip_is_counted() {
        // The control: following links unconditionally is how a link back to an
        // ancestor makes a walk unbounded, so the default must still refuse —
        // and the refusal must be a number rather than a silent `continue`.
        let root = tmp("nofollowlinks");
        write(&root, "plain.gb", "x");
        write(&root, "realdir/a.gb", "x");
        link_dir(&crate::abs(&root, "realdir"), &crate::abs(&root, "linkdir"));

        let (found, report) = walk(&root, &WalkOptions::default());
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["plain.gb", "realdir/a.gb"]);
        assert_eq!(
            report.links_skipped, 1,
            "a skipped link must be countable, not invisible"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_link_we_were_asked_to_follow_and_could_not_resolve_is_incomplete() {
        // The link is the road to the files, not the files. When the road goes
        // away — a `subst` drive unmapped, a disk unplugged, a share that
        // blinked — the plasmids behind it are untouched, and a walk that
        // reports "I found one file" rather than "I could not look" hands the
        // caller a mass deletion: measured before the fix, `2 removed` and
        // `#!complete 1` written for two `.gb` files still on disk byte for
        // byte, after which `pl library` had nothing to warn about and
        // `pl find GAATTC` answered "not in my library" for both.
        let root = tmp("danglinglink");
        write(&root, "plain.gb", "x");
        let target = tmp("danglinglink-target");
        write(&target, "a.gb", "x");
        link_dir(&target, &crate::abs(&root, "linked"));
        // The link stays; only what it points at goes away.
        std::fs::remove_dir_all(&target).unwrap();

        let opts = WalkOptions {
            follow_links: true,
            ..Default::default()
        };
        let (found, report) = walk(&root, &opts);
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["plain.gb"], "the linked sub-tree is missing");
        assert!(
            report.incomplete.is_some(),
            "a link we were told to follow and could not resolve is a place we \
             did not look, not a place we looked and found nothing: {:?}",
            report.errors
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The same hole on the other arm, which needs no `--follow-links` at all.
    ///
    /// Unix-only by necessity rather than by choice: on Windows
    /// `DirEntry::metadata` returns what `FindNextFileW` already handed back
    /// and cannot fail, so there is no way to reach this arm there.
    #[cfg(unix)]
    #[test]
    fn an_entry_that_cannot_be_stat_ed_is_incomplete_and_keeps_the_real_error() {
        use std::os::unix::fs::PermissionsExt;
        // Readable but not searchable. `read_dir` lists every child and `lstat`
        // on each of them fails with EACCES, so an entire directory's worth of
        // rows used to be dropped with `incomplete` left unset.
        let root = tmp("nostat");
        write(&root, "keep.gb", "x");
        write(&root, "locked/a.gb", "x");
        let locked = crate::abs(&root, "locked");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();

        // Root ignores the missing `x` bit, and a test that silently stops
        // testing under a root CI is a check that cannot fail.
        let blocked = std::fs::metadata(crate::abs(&root, "locked/a.gb")).is_err();
        let (found, report) = walk(&root, &WalkOptions::default());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(
            blocked,
            "this test cannot exercise the stat failure as root — run it unprivileged"
        );
        let rels: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["keep.gb"], "the locked file was not reached");
        assert!(
            report.incomplete.is_some(),
            "every row under an unreadable directory would be dropped: {:?}",
            report.errors
        );
        let reason = &report.errors[0].1;
        assert!(
            reason.contains("os error 13"),
            "the operator needs the EACCES, not the word \"metadata\": {reason:?}"
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

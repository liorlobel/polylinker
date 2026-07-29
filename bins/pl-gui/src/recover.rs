//! Autosave and crash recovery.
//!
//! # Two rules, and everything else follows
//!
//! **Never touch the file the user opened.** Autosave writes only into the
//! recovery directory. An editor that silently rewrites the original every few
//! minutes has turned "I'll close without saving" into a lie, and there is no
//! way to get the old bytes back. The user's file changes when the user says so.
//!
//! **The recovery file must be readable without this program.** If the app
//! crashes on a particular molecule it will crash again on reopening it, and
//! recovery that requires the crashing app is worth very little. So the format
//! is a short plain-text header followed by an ordinary GenBank record: anyone
//! can open it in a text editor, delete the first six lines, and have their
//! sequence back.
//!
//! # Presence means an unclean exit
//!
//! There is no "was it a crash?" flag to get wrong. Quitting cleanly deletes
//! the file; if one is there at startup, the process that wrote it did not
//! finish. Nothing needs to be recorded for that to work, which is the point —
//! a flag written *during* a crash is a flag that does not get written.
//!
//! # Two copies of the app
//!
//! Recovery files are named per process, so a second window cannot overwrite
//! the first one's — two live processes always hold distinct PIDs. A *dead*
//! process is another matter: the OS reissues its PID, and the name alone then
//! cannot tell "my own file" from "a crashed session that happened to hold my
//! number". So the name is only half the rule and [`claim`] is the other half:
//! a slot that already exists is somebody's unclean exit, whatever it is called,
//! and this run takes the next free one.
//!
//! On startup every stale file is *listed*, with its title and age, rather than
//! one being picked. Guessing which is the interesting one is how the wrong
//! draft gets restored, and the user knows and the program does not.

use std::path::{Path, PathBuf};

/// One autosaved document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The file it came from. `None` for a document that was never saved —
    /// which is exactly the one that most needs recovering.
    pub original: Option<PathBuf>,
    pub title: String,
    /// Seconds since the Unix epoch.
    pub saved_at: u64,
    /// Edits in the log when this was written. Shown so a user choosing
    /// between two recovery files can tell which is further along.
    pub ops: usize,
    /// The molecule, as GenBank.
    pub genbank: String,
}

const MAGIC: &str = "polylinker-recovery 1";
const SEPARATOR: &str = "--";

/// Encode a snapshot.
///
/// The header is one `key: value` per line. Values are escaped in a **single
/// pass** — a path can legitimately contain a backslash, and a chain of
/// `replace` calls turns `C:\temp\thing` into a tab and two lost directories.
/// That has bitten this project twice, in two different codecs, so it is
/// written the same careful way here.
pub fn encode(s: &Snapshot) -> String {
    let mut out = String::with_capacity(s.genbank.len() + 256);
    out.push_str(MAGIC);
    out.push('\n');
    out.push_str(&format!(
        "original: {}\n",
        escape(
            &s.original
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        )
    ));
    out.push_str(&format!("title: {}\n", escape(&s.title)));
    out.push_str(&format!("saved: {}\n", s.saved_at));
    out.push_str(&format!("ops: {}\n", s.ops));
    out.push_str(SEPARATOR);
    out.push('\n');
    out.push_str(&s.genbank);
    out
}

/// Decode a snapshot, or say what is wrong with it.
///
/// A damaged header does **not** discard the body: a truncated or corrupted
/// recovery file still holds the sequence, and refusing to hand it back because
/// a timestamp did not parse would be the cruellest possible failure. See
/// [`salvage`].
pub fn decode(text: &str) -> Result<Snapshot, String> {
    let mut lines = text.lines();
    if lines.next() != Some(MAGIC) {
        return Err(format!("not a recovery file: expected {MAGIC:?} on line 1"));
    }
    let (mut original, mut title, mut saved, mut ops) = (None, String::new(), 0u64, 0usize);
    let mut body_at = None;
    let mut consumed = MAGIC.len() + 1;
    for line in lines {
        consumed += line.len() + 1;
        if line == SEPARATOR {
            body_at = Some(consumed);
            break;
        }
        let Some((k, v)) = line.split_once(": ") else {
            // A bare key with an empty value is legal and common.
            if let Some(k) = line.strip_suffix(':') {
                if k == "original" {
                    original = None;
                }
            }
            continue;
        };
        let v = unescape(v);
        match k {
            "original" => original = if v.is_empty() { None } else { Some(v.into()) },
            "title" => title = v,
            "saved" => saved = v.parse().unwrap_or(0),
            "ops" => ops = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    let Some(at) = body_at else {
        return Err("no '--' separator: the header never ended".into());
    };
    Ok(Snapshot {
        original,
        title,
        saved_at: saved,
        ops,
        genbank: text.get(at..).unwrap_or("").to_string(),
    })
}

/// Everything after the first `--`, whatever state the header is in.
///
/// The last resort, and the reason the format is plain text: if the header is
/// unreadable the sequence is still there, and handing back a molecule with a
/// missing title beats handing back nothing.
pub fn salvage(text: &str) -> Option<&str> {
    let mut off = 0;
    for line in text.lines() {
        off += line.len() + 1;
        if line == SEPARATOR {
            return text.get(off..);
        }
    }
    None
}

/// Escape one header value, in a single pass over the input.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// The inverse, also single-pass.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            // An unknown escape is kept as written rather than swallowed: a
            // Windows path is full of backslashes, and eating one silently
            // changes where the file came from.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Where recovery files live.
///
/// Beside the index cache, under a directory of their own so a user can find
/// and delete them without touching anything else.
pub fn recovery_dir() -> Result<PathBuf, String> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is not set")?
            .join("Polylinker")
    } else if cfg!(target_os = "macos") {
        home()?.join("Library/Application Support/Polylinker")
    } else {
        match std::env::var_os("XDG_STATE_HOME") {
            Some(v) => PathBuf::from(v).join("polylinker"),
            None => home()?.join(".local/state/polylinker"),
        }
    };
    let dir = base.join("recovery");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

/// The recovery file for this process and document slot.
pub fn recovery_path(dir: &Path, slot: usize) -> PathBuf {
    dir.join(format!("{}-{slot}.recover", std::process::id()))
}

/// A recovery file, with whatever could be read out of it.
///
/// Named because [`stale`] and [`claim`] both hand these back and the pair is
/// otherwise deep enough to trip `clippy::type_complexity`.
pub type Found = (PathBuf, Result<Snapshot, String>);

/// How many slots [`claim`] will look at before giving up.
///
/// [`claim`] probes names instead of listing the directory, so nothing else
/// bounds the search: a directory that somehow held every slot name would turn
/// startup into an open-ended run of `stat` calls. Sixty-four is far past the
/// realistic case, which is one — a single crashed session that held this PID.
const MAX_SLOTS: usize = 64;

/// This run's recovery file, plus anything already sitting at a name this
/// process would use.
///
/// `std::process::id()` identifies a *run* only while that run is alive. A file
/// left behind by a dead process that held the same PID is indistinguishable by
/// name from our own in-progress one, and then both halves of the PID
/// convention turn against the user at once: [`stale`] skips it because the name
/// starts with our prefix, so the Recover banner never lists it, and
/// [`recovery_path`] resolves to that same file, so [`clear`] deletes it on the
/// next clean quit — which needs no document to have been opened at all — and
/// the first autosave renames over it. A crashed session's draft was hidden and
/// then destroyed, with no banner, no warning and no copy.
///
/// So *existence* decides, not the name. Anything already at one of our slots
/// belongs to a session that did not close cleanly: it is returned to be listed
/// beside the rest, and this run takes the first free slot instead. Two
/// concurrent windows still cannot collide, because they hold distinct PIDs and
/// so never look at the same names.
///
/// `None` for the path means every slot is taken and there is nowhere left to
/// write; the caller must say autosave is off rather than overwrite one of the
/// files it has just promised to list.
pub fn claim(dir: &Path) -> (Option<PathBuf>, Vec<Found>) {
    let mut found = Vec::new();
    for slot in 0..MAX_SLOTS {
        let p = recovery_path(dir, slot);
        // `try_exists` rather than `exists`: a name we cannot stat is one we
        // must not assume is free, because writing there would overwrite it.
        if matches!(p.try_exists(), Ok(false)) {
            return (Some(p), found);
        }
        match std::fs::read_to_string(&p) {
            Ok(t) => found.push((p, decode(&t))),
            Err(e) => found.push((p, Err(e.to_string()))),
        }
    }
    (None, found)
}

/// Write a snapshot atomically.
///
/// Temp file, flush, `sync_all`, rename. Durability before visibility: a rename
/// that lands before the data does leaves a recovery file that exists and is
/// empty, which is worse than none because it looks like a recovery.
pub fn write(path: &Path, snap: &Snapshot) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension("recover.tmp");
    let body = encode(snap);
    let go = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.flush()?;
        f.sync_all()?;
        Ok(())
    };
    if let Err(e) = go() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", tmp.display()));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Delete this process's recovery file. Called on a clean quit.
///
/// Its absence is what says the exit was clean, so this must run on the normal
/// path out — and nothing else needs to be written for that to be true.
pub fn clear(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Recovery files left by processes other than this one.
///
/// Returned with what could be read from each, newest first. A file whose
/// header will not parse is still returned, with whatever [`salvage`] found,
/// because the sequence is the part that matters.
pub fn stale(dir: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let me = format!("{}-", std::process::id());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().map(|n| n.to_string_lossy().to_string());
        let Some(name) = name else { continue };
        if !name.ends_with(".recover") || name.starts_with(&me) {
            continue;
        }
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(e) => {
                out.push((p, Err(e.to_string())));
                continue;
            }
        };
        out.push((p, decode(&text)));
    }
    out.sort_by_key(|(p, s)| {
        (
            std::cmp::Reverse(s.as_ref().map(|s| s.saved_at).unwrap_or(0)),
            p.clone(),
        )
    });
    out
}

/// Which application the OS will open a file extension with.
///
/// Read-only, and deliberately so. Claiming `.dna` at install time is how two
/// plasmid editors end up fighting over double-click, and doing it without
/// asking is worse than not doing it. Reporting who currently owns the
/// extension lets the app *say* "SnapGene opens these" and leave the decision
/// where it belongs.
/// Ask the shell without flashing a console window over the app.
///
/// `CREATE_NO_WINDOW`. Without it a windowed build pops a black cmd.exe window
/// in front of the user for as long as `assoc` takes to run — which looked like
/// the app launching something, on the welcome screen, unprompted.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Read once per process by the caller. This spawns a child process and blocks
/// until it exits, so it must never be called from a paint closure.
#[cfg(windows)]
pub fn association(ext: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("cmd")
        .args(["/C", "assoc", &format!(".{ext}")])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let handler = s.split_once('=').map(|(_, v)| v.trim().to_string())?;
    if handler.is_empty() {
        return None;
    }
    Some(handler)
}

#[cfg(not(windows))]
pub fn association(_ext: &str) -> Option<String> {
    // No portable equivalent, and guessing from a desktop file would be a
    // different answer from the one the desktop actually uses.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            original: Some(PathBuf::from(r"C:\temp\thing\pUC19.dna")),
            title: "pUC19".into(),
            saved_at: 1_785_000_000,
            ops: 7,
            genbank: "LOCUS       x  10 bp DNA circular SYN\nORIGIN\n        1 acgtacgtac\n//\n"
                .into(),
        }
    }

    #[test]
    fn a_snapshot_survives_the_round_trip() {
        let s = snap();
        assert_eq!(decode(&encode(&s)).unwrap(), s);
    }

    #[test]
    fn a_windows_path_comes_back_as_itself() {
        // The bug this file is written to avoid, and which this project has
        // shipped twice in other codecs: chained replaces turn `C:\temp\thing`
        // into a tab and two lost directories, so the recovery file forgets
        // which document it belongs to.
        for p in [
            r"C:\temp\thing\a.dna",
            r"C:\new\repo\test.gb",
            r"\\server\share\x.gb",
            "/home/lior/plasmids/x.gb",
            r"C:\a\\b\c",
        ] {
            let mut s = snap();
            s.original = Some(PathBuf::from(p));
            let back = decode(&encode(&s)).unwrap();
            assert_eq!(back.original, Some(PathBuf::from(p)), "{p}");
        }
    }

    #[test]
    fn a_newline_in_a_title_cannot_forge_a_header() {
        // A title is user data. Unescaped, one containing "\nops: 999" would
        // rewrite a field, and one containing "\n--" would end the header early
        // and truncate the sequence.
        let mut s = snap();
        s.title = "evil\nops: 999\n--\nnot the real sequence".into();
        let back = decode(&encode(&s)).unwrap();
        assert_eq!(back.title, s.title);
        assert_eq!(back.ops, 7, "the forged field did not take");
        assert_eq!(back.genbank, s.genbank, "and the body is intact");
    }

    #[test]
    fn an_unsaved_document_records_that_it_has_no_file() {
        // The document that most needs recovering is the one never written to
        // disk, so an absent path must round-trip as absent rather than as "".
        let mut s = snap();
        s.original = None;
        let back = decode(&encode(&s)).unwrap();
        assert_eq!(back.original, None);
    }

    #[test]
    fn the_sequence_is_recoverable_even_when_the_header_is_not() {
        // The whole reason the format is plain text. A truncated or mangled
        // header must not cost the user their sequence.
        let text = encode(&snap());
        let broken = text.replacen("saved: 1785000000", "saved: \u{fffd}garbage", 1);
        let s = decode(&broken).expect("a bad timestamp is not fatal");
        assert_eq!(s.saved_at, 0, "unknown rather than invented");
        assert_eq!(s.genbank, snap().genbank);

        // And if even the magic line is gone, salvage still finds the body.
        let headless = text.replacen(MAGIC, "corrupted", 1);
        assert!(decode(&headless).is_err());
        assert_eq!(salvage(&headless), Some(snap().genbank.as_str()));
    }

    #[test]
    fn something_that_is_not_a_recovery_file_is_refused_by_name() {
        let e = decode("LOCUS x\nORIGIN\n//\n").unwrap_err();
        assert!(e.contains("not a recovery file"), "{e}");
        assert!(decode("").is_err());
        let e = decode(&format!("{MAGIC}\ntitle: x\n")).unwrap_err();
        assert!(e.contains("separator"), "{e}");
    }

    #[test]
    fn a_body_containing_the_separator_is_not_cut_short() {
        // GenBank has no '--' line, but a COMMENT can hold anything, and only
        // the *first* separator ends the header.
        let mut s = snap();
        s.genbank = "LOCUS x\nCOMMENT\n--\nmore\n//\n".into();
        let back = decode(&encode(&s)).unwrap();
        assert_eq!(back.genbank, s.genbank);
    }

    #[test]
    fn an_empty_sequence_is_still_a_valid_snapshot() {
        let mut s = snap();
        s.genbank = String::new();
        assert_eq!(decode(&encode(&s)).unwrap(), s);
    }

    #[test]
    fn recovery_paths_differ_between_slots_and_carry_the_process() {
        let d = Path::new("/tmp/x");
        assert_ne!(recovery_path(d, 0), recovery_path(d, 1));
        let name = recovery_path(d, 0)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            name.starts_with(&format!("{}-", std::process::id())),
            "{name}"
        );
        assert!(name.ends_with(".recover"));
    }

    #[test]
    fn writing_and_reading_back_goes_through_the_filesystem() {
        let dir = std::env::temp_dir().join(format!("pl-recover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.recover");
        write(&p, &snap()).unwrap();
        let back = decode(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(back, snap());

        // A clean quit removes it, and its absence is what says the exit was
        // clean.
        clear(&p);
        assert!(!p.exists());
        clear(&p); // and clearing twice is not an error
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn this_process_does_not_offer_to_recover_its_own_file() {
        // Two windows open at once must not each try to restore the other's
        // work-in-progress as if it were a crash.
        let dir = std::env::temp_dir().join(format!("pl-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mine = recovery_path(&dir, 0);
        write(&mine, &snap()).unwrap();
        assert!(stale(&dir).is_empty(), "our own file is not stale");

        let theirs = dir.join("999999-0.recover");
        let mut other = snap();
        other.title = "from another process".into();
        write(&theirs, &other).unwrap();
        let found = stale(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1.as_ref().unwrap().title, "from another process");

        // Newest first, so the list reads the way a person would expect.
        let older = dir.join("999998-0.recover");
        let mut old = snap();
        old.saved_at = 1;
        old.title = "older".into();
        write(&older, &old).unwrap();
        let found = stale(&dir);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].1.as_ref().unwrap().title, "from another process");
        assert_eq!(found[1].1.as_ref().unwrap().title, "older");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_draft_left_at_our_own_pid_is_listed_and_not_written_over() {
        // PID reuse. A crashed session left `{pid}-0.recover`; the OS hands the
        // same number to this run. `stale` skips the file because the name
        // begins with our prefix, so the banner never lists it, and
        // `recovery_path(dir, 0)` resolves to it, so `clear` on the next clean
        // quit deletes it and the first autosave renames over it. Total, silent
        // loss of exactly the artefact this module exists to protect.
        let dir = std::env::temp_dir().join(format!("pl-claim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let crashed = recovery_path(&dir, 0);
        let mut draft = snap();
        draft.title = "40 unsaved edits".into();
        draft.ops = 40;
        write(&crashed, &draft).unwrap();

        let (path, mine) = claim(&dir);
        let path = path.expect("slot 1 is free");
        assert_ne!(
            path, crashed,
            "this run must not write where the crashed one already did"
        );
        assert_eq!(mine.len(), 1, "and the draft is handed back to be listed");
        assert_eq!(mine[0].0, crashed);
        assert_eq!(mine[0].1.as_ref().unwrap().title, "40 unsaved edits");

        // The whole point: quitting cleanly, having opened nothing, no longer
        // deletes it.
        clear(&path);
        assert!(crashed.exists(), "the crashed draft survives a clean quit");
        assert_eq!(
            decode(&std::fs::read_to_string(&crashed).unwrap())
                .unwrap()
                .ops,
            40
        );

        // A second collision takes the next slot again, and both are listed.
        write(&path, &draft).unwrap();
        let (third, mine) = claim(&dir);
        assert_eq!(third.unwrap(), recovery_path(&dir, 2));
        assert_eq!(mine.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_recovery_directory_still_gives_this_run_slot_zero() {
        // The ordinary case must not drift: no collision means no change.
        let dir = std::env::temp_dir().join(format!("pl-claim0-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (path, mine) = claim(&dir);
        assert_eq!(path.unwrap(), recovery_path(&dir, 0));
        assert!(mine.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_recovery_file_is_listed_rather_than_hidden() {
        // A file that will not parse is the one a user most needs to be told
        // about, so it appears in the list with its error instead of vanishing.
        let dir = std::env::temp_dir().join(format!("pl-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("999997-0.recover"), "not a recovery file at all").unwrap();
        let found = stale(&dir);
        assert_eq!(found.len(), 1);
        assert!(found[0].1.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

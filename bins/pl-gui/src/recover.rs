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
//! can open it in a text editor, delete everything down to and including the
//! `--` line, and have their sequence back.
//!
//! Down to the `--`, and NOT "the first six lines", which is what this said
//! while the header had grown optional keys. `exit:` is written only for a
//! deliberate quit and `unsaved:` only when that count exists, so the header
//! runs to six, seven or eight lines and the six-line instruction eats the
//! LOCUS line of every file that carries either one. A rescue instruction that
//! is right on some files and destroys the first record of others is worse than
//! no instruction at all, because the reader following it has no reason to
//! doubt it.
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
    ///
    /// EVERY edit in the document, saved or not — `OpLog::path().len()`. It is
    /// a good number to *show* and the wrong number to *decide* on; see
    /// [`Snapshot::unsaved`], which exists because it was decided on.
    pub ops: usize,
    /// How many of those edits are in no file — `Document::unsaved_ops()` at
    /// the moment this was written — or `None` when that count does not exist.
    ///
    /// THE RECOVER BANNER USED TO TAKE ITS ONE DECISION-SUPPORT LINE ON `ops`,
    /// and `ops` is not the quantity that sentence is about. A document opened,
    /// edited, SAVED, and then lost to a crash has `ops > 0` with nothing left
    /// over, so `draft_age`'s "this draft holds nothing the file does not"
    /// escape hatch never fired for it and the line fell through to comparing
    /// the two timestamps — which is a comparison the draft always wins: the
    /// autosave heartbeat restamps every draft once per thirty seconds whether
    /// or not the molecule moved, and nothing clears a draft when its document
    /// is saved, so `saved_at` walks steadily past the file's mtime. Every
    /// crash draft of a saved file was therefore advertised as "written after
    /// the file on disk was last saved", inviting the user to replace a rich
    /// `.dna` with a GenBank rendering of the same molecule that has lost the
    /// container, the typed primers and the methylation flags to say it.
    ///
    /// `None` means "no answer", never zero, and it has two sources. A file
    /// written by a build older than this key carries no `unsaved:` line at
    /// all; and a document whose saved cursor is not an ancestor of the current
    /// one — save, then seek onto another branch — has no distance to report,
    /// which is exactly what `Document::unsaved_ops` returns `None` for. Both
    /// fall back to `ops` in `draft_age`, which is what the banner did before
    /// this key existed, so an old draft reads exactly as it always did.
    ///
    /// Additive in both directions, for [`Snapshot::abandoned`]'s reason and by
    /// the same mechanism: [`decode`] ignores keys it does not know, so an old
    /// build reading a new file loses only this number, and a new build reading
    /// an old file sees the `None` that is the truth about it. The MAGIC line
    /// does not move for a key that cannot make either build misread a
    /// molecule.
    pub unsaved: Option<usize>,
    /// The user was asked about these edits and chose to abandon them.
    ///
    /// The module doc above argues there must be no crash flag, because "a flag
    /// written during a crash is a flag that does not get written". That
    /// argument survives a flag written on a **clean** exit, and only that: this
    /// one is written by the unsaved-changes guard's "close without saving"
    /// path, on the way out, with nothing crashing. The invariant is unchanged
    /// — presence of the FILE still means work was left behind, and absence of
    /// this KEY still reads as a crash, which is what an old file written by an
    /// older build correctly says. Do not delete it as a violation of a rule it
    /// does not violate: without it a deliberately abandoned draft comes back
    /// under "A previous session did not close cleanly", which tells a user
    /// their app crashed when it did not.
    pub abandoned: bool,
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
    // Written only when there is an answer. An absent key is "unknown", which
    // is what a document sitting on a branch its last save is not an ancestor
    // of genuinely is; writing a 0 there would be inventing the most dangerous
    // possible value, since 0 is what tells the banner the draft holds nothing
    // the file does not.
    if let Some(n) = s.unsaved {
        out.push_str(&format!("unsaved: {n}\n"));
    }
    if s.abandoned {
        out.push_str("exit: unsaved\n");
    }
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
    let mut unsaved: Option<usize> = None;
    let mut abandoned = false;
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
            // `.ok()`, not `.unwrap_or(0)`: a value that will not parse is
            // unknown, and the 0 the other two keys fall back to is the one
            // number this field must never be invented as — it is what tells
            // the banner the draft holds nothing the file does not.
            "unsaved" => unsaved = v.parse().ok(),
            // Unknown keys are ignored here, which is what makes this key
            // compatible in both directions: an old file read by a new build
            // reads as a crash, and a new file read by an old build loses only
            // the label. `unsaved` above rides on the same rule.
            "exit" => abandoned = v == "unsaved",
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
        unsaved,
        abandoned,
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
///
/// `pub(crate)` so `session.rs` uses this one rather than growing a second. A
/// chain of `replace` calls turns `C:\temp\thing` into a tab and two lost
/// directories; that has bitten this project twice already, and a copy of the
/// careful version is a copy that can be edited into the careless one.
pub(crate) fn escape(s: &str) -> String {
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
pub(crate) fn unescape(s: &str) -> String {
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
    let dir = state_base()?.join("recovery");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// This user's directory for everything the app remembers between runs.
///
/// Split out of [`recovery_dir`] so `settings.rs` can write *beside* the
/// recovery directory rather than inside it. Not inside: [`stale`] filters on
/// `.ends_with(".recover")`, so a stray file there would in fact be ignored,
/// but that directory is documented as somewhere a user can find and delete
/// crash drafts "without touching anything else" — and clearing crash drafts
/// should not silently reset a window layout.
///
/// Creates nothing; the two callers create what they need.
pub fn state_base() -> Result<PathBuf, String> {
    if cfg!(windows) {
        Ok(std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is not set")?
            .join("Polylinker"))
    } else if cfg!(target_os = "macos") {
        Ok(home()?.join("Library/Application Support/Polylinker"))
    } else {
        match std::env::var_os("XDG_STATE_HOME") {
            Some(v) => Ok(PathBuf::from(v).join("polylinker")),
            None => Ok(home()?.join(".local/state/polylinker")),
        }
    }
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

/// How many slots [`claim`] and [`claim_next`] look at.
///
/// Both probe names instead of listing the directory, so nothing else bounds
/// the search: a directory that somehow held every slot name would turn startup
/// into an open-ended run of `stat` calls. Sixty-four is far past the realistic
/// case, which is one — a single crashed session that held this PID.
///
/// [`claim`] now pays all sixty-four on EVERY launch rather than stopping at
/// the first free name, and that is deliberate: stopping early is what hid a
/// crashed session's draft from the Recover banner whenever it sat at any slot
/// but the lowest free one. Sixty-four `try_exists` calls against a directory
/// that holds at most a handful of small files is a few hundred microseconds
/// once per run, against a draft the user is never told about — see [`claim`].
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
/// **EVERY SLOT IS LOOKED AT, AND THE FREE ONE IS ANSWERED LAST.** This used to
/// `return` at the first free name, so the second value listed only a
/// *contiguous prefix* of the occupied slots and a draft with any lower slot
/// free was never seen by anything: [`stale`] skips it because the name carries
/// this process's prefix, and `claim` stopped before reaching it. Slot
/// holes are the ordinary case, not a contrivance — `App::close_tab` frees the
/// slot of a tab closed cleanly and nothing ever compacts them, so a user who
/// crashed with two windows open was offered one draft and silently not the
/// other. Nothing was destroyed by that (`claim_next` refuses any slot whose
/// file exists, and `clear` is only ever handed a path this run was issued),
/// but a crash draft nobody is told about is a crash draft nobody restores.
///
/// `None` for the path means every slot is taken and there is nowhere left to
/// write; the caller must say autosave is off rather than overwrite one of the
/// files it has just promised to list.
pub fn claim(dir: &Path) -> (Option<PathBuf>, Vec<Found>) {
    let mut found = Vec::new();
    let mut free: Option<usize> = None;
    for slot in 0..MAX_SLOTS {
        let p = recovery_path(dir, slot);
        // `try_exists` rather than `exists`: a name we cannot stat is one we
        // must not assume is free, because writing there would overwrite it.
        if matches!(p.try_exists(), Ok(false)) {
            // REMEMBERED, NOT RETURNED — the whole of the fix. `get_or_insert`
            // keeps the LOWEST free slot, which is what a `return` here used to
            // hand back, while the walk carries on to the slots above it.
            free.get_or_insert(slot);
        } else {
            match std::fs::read_to_string(&p) {
                Ok(t) => found.push((p, decode(&t))),
                Err(e) => found.push((p, Err(e.to_string()))),
            }
        }
    }
    (free.map(|slot| recovery_path(dir, slot)), found)
}

/// A free recovery slot for this process, avoiding the ones already handed out.
///
/// [`claim`] decides by EXISTENCE, which is exactly right for another run's
/// leftovers and exactly wrong for our own. A slot given to a tab that has not
/// autosaved yet is not on disk, so `claim` would hand the same name to the next
/// tab and the two would write over each other — one draft on disk for two
/// documents, with the loser's edits gone and nothing saying so. The names this
/// process is already holding are therefore passed in rather than inferred.
///
/// `None` when every slot is taken. The caller must then say autosave is off for
/// that document rather than overwrite a file it has promised to list.
pub fn claim_next(dir: &Path, held: &[PathBuf]) -> Option<PathBuf> {
    (0..MAX_SLOTS).map(|s| recovery_path(dir, s)).find(|p| {
        // `try_exists` rather than `exists`, for the same reason as `claim`: a
        // name we cannot stat is one we must not assume is free.
        !held.contains(p) && matches!(p.try_exists(), Ok(false))
    })
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

/// How recently a draft must have been written to be treated as maybe-live.
///
/// Three times `AUTOSAVE_EVERY`. A running window restamps every draft it holds
/// every thirty seconds — see `App::autosave`'s heartbeat, which exists to make
/// this sentence true — so anything inside ninety belongs to a session that is
/// very probably still going; anything older belongs to one that stopped. Three
/// periods and not one, so that a heartbeat delayed by a slow disk, a blocked
/// frame or a suspended laptop does not read as a death.
pub const LIVE_WINDOW_SECS: u64 = 90;

/// Might this draft belong to a Polylinker that is still running?
///
/// `stale` returns every `*.recover` that is not this process's, and NOTHING
/// checks whether the process named in the filename is alive — there is no lock
/// file and no PID probe anywhere in this module. So a second window lists the
/// first window's live drafts under "A previous session did not close cleanly",
/// and its Discard button permanently deletes the running session's only crash
/// copy while the user is still typing into it.
///
/// This is a heuristic and is deliberately not dressed up as anything else. A
/// PID probe would be exact and needs a platform crate in a binary whose whole
/// posture is that it has almost none; freshness needs nothing and errs by
/// occasionally calling a very recent crash "maybe live", which costs the user
/// one deferred Discard and no data at all.
///
/// **THE CADENCE IS A REQUIREMENT ON THE WRITER, NOT AN OBSERVATION ABOUT IT.**
/// This doc used to say a live session could not be called dead because of "the
/// writer's own thirty-second cadence", and the writer had none: `App::autosave`
/// skips any tab whose `(path, title, cursor)` memo is unchanged, so a window
/// whose user stopped typing stopped stamping and its `saved_at` froze at the
/// last edit. Ninety seconds of thinking was enough to move a running window's
/// only crash copy into "A previous session did not close cleanly" in a second
/// window, with Discard enabled — and the owner would not write it again,
/// because the memo still matched. `App::autosave` now restamps every draft it
/// holds once per period whether or not the molecule moved, and
/// `an_idle_windows_draft_keeps_saying_it_is_alive` is what holds it to that. If
/// that heartbeat is ever removed, this function goes back to being wrong in the
/// one direction it is not allowed to be wrong in.
///
/// The trade this makes is deliberate: liveness is claimed by a file the owner
/// keeps rewriting, so a *genuinely* crashed draft still ages out after
/// [`LIVE_WINDOW_SECS`] and stays discardable. Nothing here can make a draft
/// undiscardable, because nothing here is a lock — only a process that is
/// running can keep saying it is running.
pub fn maybe_live(saved_at: u64, now: u64) -> bool {
    saved_at != 0 && now.saturating_sub(saved_at) <= LIVE_WINDOW_SECS
}

/// How long a `.recover.tmp` must have gone untouched before it is swept.
///
/// One hour, the same threshold `pl_scan::store::sweep_stale_temps` uses for
/// the index's temporaries. The number does one job: tell a temp file a live
/// window is part way through writing from one a dead window will never finish.
/// [`write`] creates its temp, writes, `sync_all`s and renames in a single
/// call, so the live case is milliseconds and even a 4.6 Mb molecule on a
/// struggling disk is nowhere near an hour — while the dead case, by
/// definition, never touches the file again. Forty times [`LIVE_WINDOW_SECS`]
/// because deleting a temp out from under a writer costs a draft, and keeping a
/// dead one an extra hour costs nothing at all.
const TEMP_SWEEP_SECS: u64 = 3600;

/// Delete recovery temporaries left behind by a run that died mid-write.
///
/// [`write`] removes its temp on both failure paths, and neither of those is
/// the event this module exists for: a crash DURING the write leaks
/// `{pid}-{slot}.recover.tmp` — up to the size of the document — and nothing
/// ever looks at it again. [`stale`] filters on `.ends_with(".recover")`, which
/// a `.recover.tmp` does not satisfy, and [`claim`] and [`claim_next`] probe
/// exact slot names, so the file is invisible to every listing path and no
/// later run reuses the PID in its name. Modelled on
/// `pl_scan::store::sweep_stale_temps`, down to the hour.
///
/// Best-effort and silent about individual failures: another process may hold
/// one open, and failing a startup scan over a stale temp would be absurd.
fn sweep_stale_temps(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let cutoff = std::time::Duration::from_secs(TEMP_SWEEP_SECS);
    for e in entries.flatten() {
        if !e.file_name().to_string_lossy().ends_with(".recover.tmp") {
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

/// Recovery files left by processes other than this one.
///
/// Returned with what could be read from each, newest first. A file whose
/// header will not parse is still returned, with whatever [`salvage`] found,
/// because the sequence is the part that matters.
///
/// Sweeps abandoned write temporaries on the way past. Here because this is the
/// one function that walks the recovery directory and it runs at startup, which
/// is also when the crash that leaked one has just happened.
pub fn stale(dir: &Path) -> Vec<Found> {
    sweep_stale_temps(dir);
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
            // Seven edits, two of which the file has never seen: the ordinary
            // shape of a crash draft of a document that HAS been saved, and the
            // one the banner used to get wrong.
            unsaved: Some(2),
            abandoned: false,
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

    /// The number the Recover banner takes its one decision on has to survive
    /// the file, and an old file has to say it does not know rather than zero.
    ///
    /// PROVEN TO FAIL at d8c218b: there was no `unsaved` key at all, and the
    /// banner decided on `ops` — every edit in the document, saved or not — so
    /// a crash draft of a document that had been saved was advertised as
    /// "written after the file on disk was last saved" with nothing in it the
    /// file did not already have. To re-break it, change the `"unsaved" =>
    /// unsaved = v.parse().ok(),` arm in `decode` to `"unsaved" => {}`.
    #[test]
    fn the_unsaved_count_survives_the_file_and_an_older_file_admits_it_cannot_say() {
        assert_eq!(decode(&encode(&snap())).unwrap().unsaved, Some(2));

        // ZERO IS A VALUE, not an absence, and it is the one the banner acts
        // on: it is what says the draft holds nothing the file does not.
        let mut filed = snap();
        filed.unsaved = Some(0);
        let text = encode(&filed);
        assert!(text.contains("\nunsaved: 0\n"), "{text}");
        assert_eq!(decode(&text).unwrap().unsaved, Some(0));

        // No answer writes no key, and comes back as no answer.
        let mut unknown = snap();
        unknown.unsaved = None;
        let text = encode(&unknown);
        assert!(!text.contains("\nunsaved: "), "{text}");
        assert_eq!(decode(&text).unwrap().unsaved, None);

        // A file written by a build older than this key: the same bytes minus
        // the one line. It must read as unknown — NOT as zero, which would
        // claim the user's file already holds work that only the draft has —
        // and everything else about it must come back untouched.
        let old = encode(&snap()).replacen("unsaved: 2\n", "", 1);
        let back = decode(&old).unwrap();
        assert_eq!(
            back.unsaved, None,
            "an old draft read as though its edits were already saved"
        );
        assert_eq!(back.ops, 7, "and the rest of the header is untouched");
        assert_eq!(back.genbank, snap().genbank);

        // A value that will not parse is unknown too, for the same reason
        // `saved_at` is: unknown rather than invented.
        let bad = encode(&snap()).replacen("unsaved: 2", "unsaved: \u{fffd}", 1);
        assert_eq!(decode(&bad).unwrap().unsaved, None);
        assert_eq!(decode(&bad).unwrap().genbank, snap().genbank);
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

    /// The freshness heuristic has one direction it is not allowed to get
    /// wrong, and that direction is the whole reason it exists.
    #[test]
    fn a_draft_written_within_one_autosave_period_is_never_called_dead() {
        // A running window rewrites its draft every 30 s, so any age a live
        // session can present must come back true — including the moment it
        // was written and the moment before the next write is due.
        for age in [0, 1, 29, 30, 31, 59, 60, LIVE_WINDOW_SECS] {
            assert!(
                maybe_live(1_000_000 - age, 1_000_000),
                "a draft {age} s old was treated as a dead session's"
            );
        }
        // Past the window it ages out, so the Discard the banner withheld
        // becomes available rather than being withheld for good.
        assert!(!maybe_live(1_000_000 - LIVE_WINDOW_SECS - 1, 1_000_000));
        assert!(!maybe_live(1_000_000 - 86_400, 1_000_000));
        // A header that would not parse leaves `saved_at` at 0 — "unknown", not
        // "the epoch". Read as an age that is a live claim only if the clock is
        // also 0, which it is not on any machine this runs on.
        assert!(!maybe_live(0, 1_000_000), "unknown is not a claim of life");
        // And a clock that went backwards — a draft stamped in the future by a
        // dual-boot or a corrected NTP step — is live, not negative.
        assert!(maybe_live(1_000_060, 1_000_000), "no underflow");
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

    /// PID reuse, at EVERY slot rather than only the lowest.
    ///
    /// PROVEN TO FAIL at d8c218b: `claim` returned at the first free name, so
    /// its second value listed only a contiguous prefix of the occupied slots.
    /// The `occupied = 0` case passed there and every other case did not —
    /// `left: 0, right: 1` on the "handed back to be listed" assertion — because
    /// slot 0 was free and the walk stopped on it. That is not a contrived
    /// arrangement: `App::close_tab` frees the slot of a tab closed cleanly and
    /// nothing compacts them, so a crash with two windows open leaves exactly
    /// this shape and the user is offered one draft and silently not the other.
    ///
    /// To re-break it, change `free.get_or_insert(slot);` in `claim` back to
    /// `return (Some(p), found);`.
    #[test]
    fn a_draft_left_at_our_own_pid_is_listed_and_not_written_over() {
        // Swept across the slot because the slot is what the defect was keyed
        // on, including the last one: `MAX_SLOTS - 1` is reachable by a user who
        // has had sixty-four documents open at once, and a bound is exactly
        // where an off-by-one lives. A helper rather than a loop body so that
        // every assertion below reads at the indent it was written at.
        for occupied in [0usize, 1, 2, 5, MAX_SLOTS - 1] {
            a_crashed_draft_at_this_slot_is_listed_and_kept(occupied);
        }
    }

    /// One slot's worth of `a_draft_left_at_our_own_pid_is_listed_and_not_written_over`.
    fn a_crashed_draft_at_this_slot_is_listed_and_kept(occupied: usize) {
        // PID reuse. A crashed session left `{pid}-K.recover`; the OS hands the
        // same number to this run. `stale` skips the file because the name
        // begins with our prefix, so the banner never lists it, and
        // `recovery_path(dir, K)` resolves to it, so `clear` on the next clean
        // quit deletes it and the first autosave renames over it. Total, silent
        // loss of exactly the artefact this module exists to protect.
        // `pl-claim-0-…` and not `pl-claim0-…`: the latter is the directory
        // `an_empty_recovery_directory_still_gives_this_run_slot_zero` owns, the
        // suite runs its tests in parallel threads of ONE process, and both
        // names are built from that one `process::id()`. Sharing it made the two
        // tests take turns deleting the directory the other was renaming into,
        // which fails as "the system cannot find the path specified" in whichever
        // one loses — a flake that says nothing about recovery at all.
        let tag = format!("pl-claim-{occupied}-{}", std::process::id());
        let dir = std::env::temp_dir().join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let crashed = recovery_path(&dir, occupied);
        let mut draft = snap();
        draft.title = "40 unsaved edits".into();
        draft.ops = 40;
        write(&crashed, &draft).unwrap();

        let (path, mine) = claim(&dir);
        let path = path.expect("a free slot remains");
        // The lowest slot the crashed draft is not sitting in: slot 0 for every
        // case but the first, and slot 1 for that one. `usize::from` of the
        // comparison rather than an `if`, which clippy reads as a bool-to-int
        // conversion written the long way. Stronger than the `assert_ne!`
        // against `crashed` this replaces, and it subsumes it: `first_free` is
        // by construction not `occupied`, so naming the slot exactly says both
        // that this run took a free one and that it was not the crashed one's.
        let first_free = usize::from(occupied == 0);
        assert_eq!(
            path,
            recovery_path(&dir, first_free),
            "slot {occupied}: this run must write at the LOWEST free slot, and never where the \
             crashed one already did"
        );
        assert_eq!(
            mine.len(),
            1,
            "slot {occupied}: the draft was not handed back to be listed, so the Recover banner \
             never mentions it"
        );
        assert_eq!(mine[0].0, crashed);
        assert_eq!(mine[0].1.as_ref().unwrap().title, "40 unsaved edits");

        // The whole point: quitting cleanly, having opened nothing, no longer
        // deletes it.
        clear(&path);
        assert!(
            crashed.exists(),
            "slot {occupied}: the crashed draft did not survive a clean quit"
        );
        assert_eq!(
            decode(&std::fs::read_to_string(&crashed).unwrap())
                .unwrap()
                .ops,
            40
        );

        // A second collision takes the next free slot again, and BOTH are
        // listed — the two-window crash, which is the case a walk that stopped
        // at the first free name could not see.
        write(&path, &draft).unwrap();
        let (third, mine) = claim(&dir);
        let next_free = (0..MAX_SLOTS)
            .find(|s| *s != occupied && *s != first_free)
            .expect("a third slot");
        assert_eq!(
            third.unwrap(),
            recovery_path(&dir, next_free),
            "slot {occupied}"
        );
        assert_eq!(
            mine.len(),
            2,
            "slot {occupied}: two crashed drafts, and the banner would show fewer"
        );

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

    /// The temp file a crash mid-write leaks is swept; the one a live window is
    /// mid-write on is not.
    ///
    /// PROVEN TO FAIL without the `sweep_stale_temps` call in `stale`: the
    /// backdated temp is still on disk afterwards, which is the leak — a file
    /// up to the size of the document, under a PID no later run reuses, that no
    /// listing path can see and nothing will ever delete.
    #[test]
    fn a_temp_left_by_a_crash_mid_write_is_swept_and_a_live_ones_is_not() {
        let dir = std::env::temp_dir().join(format!("pl-tmpsweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // What `write` leaves if the process dies between `File::create` and
        // the rename.
        let leaked = dir.join("999993-0.recover.tmp");
        let live = dir.join("999992-0.recover.tmp");
        for p in [&leaked, &live] {
            std::fs::write(p, "polylinker-recovery 1\noriginal: \n").unwrap();
        }
        // Backdated past the threshold rather than slept past: an hour is the
        // point of the threshold and a test may not take one.
        let f = std::fs::File::options().write(true).open(&leaked).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(
            std::time::SystemTime::now() - std::time::Duration::from_secs(TEMP_SWEEP_SECS + 60),
        ))
        .unwrap();
        drop(f);

        // Neither is a recovery file to list, before or after.
        assert!(stale(&dir).is_empty(), "a temp was offered as a draft");
        assert!(
            !leaked.exists(),
            "the leaked temp is still there, and always would be"
        );
        assert!(
            live.exists(),
            "a temp another window is part way through writing was deleted, \
             which costs that window the draft it was saving"
        );
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

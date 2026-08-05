//! Which documents were open, so the next launch can put them back.
//!
//! Stage 2's second half, and the whole of what "always be there" asked for: no
//! project file, no Save Workspace, no dialog. You close Polylinker with six
//! plasmids on the bench and you open it with six plasmids on the bench.
//!
//! # What this is NOT
//!
//! It is not a recovery file and it holds no molecule. Every path in it names a
//! file that already exists on disk, unchanged, put there by the user. Losing
//! this file costs a tab list and nothing else, which is why — unlike
//! `recover.rs`, whose absence is a crash flag — an unreadable or missing one is
//! silent. A tab list is not the user's data.
//!
//! That is also why a document with no path is not in here. There is nowhere to
//! point at, and inventing a copy of the molecule would make this a second,
//! subtly different recovery mechanism sitting beside the real one. They are
//! COUNTED instead, so the line that reports the restore can say what it could
//! not do rather than quietly restoring four of six.
//!
//! # One file per process, claimed by rename
//!
//! Written under `session-{pid}` while the window runs, so a crash leaves the
//! list behind exactly as a clean exit does — the two cases need no telling
//! apart, because unlike a recovery draft there is nothing here to warn about.
//!
//! A second window must not restore the first window's tabs, and the same
//! freshness rule that guards a live recovery draft answers it: a session file
//! rewritten in the last minute or two belongs to a window that is still
//! running, and is left alone. See [`crate::recover::maybe_live`], including its
//! note on why freshness rather than a PID probe.
//!
//! That rule is about OTHER windows and is applied only to their names. A list
//! sitting at `session-{our own pid}` when this run has not written one is a
//! dead run's — nothing else can be holding this PID — and [`claim`] takes it
//! however fresh it looks, exactly as [`crate::recover::claim`] does with a
//! recovery slot.
//!
//! # Claimed exclusively, and not by the obvious means
//!
//! Two windows launched together — a user double-clicking two files, or a shell
//! opening a folder — both read the same list and both restore it: twelve tabs,
//! two of everything, and no telling which copy you edited. Something has to
//! make exactly one of them the winner.
//!
//! It is `create_new` — `O_EXCL` / `CREATE_NEW` — on a marker named after the
//! list. **It was a rename, and the rename did not hold.**
//! `only_one_of_eight_windows_starting_at_once_gets_the_bench` was written to
//! prove the claim exclusive and instead measured, on Windows, eight threads all
//! getting `Ok(())` from `fs::rename` of the same source, five of them going on
//! to restore the same bench. Giving each caller its own destination made it
//! pass on an idle machine and fail at two winners under full-suite load, which
//! is the worse failure of the two: a race that looks fixed. `create_new` is
//! exclusive on both platforms and has held every round since.
//!
//! The lesson is the one this project keeps relearning. "rename is atomic" is
//! true and was not the property needed, and the only reason the difference ever
//! surfaced is that the test ran eight callers at once instead of two in a row.

use std::path::{Path, PathBuf};

use crate::recover::{escape, unescape};

const HEADER: &str = "polylinker-session 1";
/// The same list, saved deliberately under a name the user chose.
///
/// A SECOND HEADER AND NOT A SECOND CODEC. A project and a session hold exactly
/// the same thing — which files were open, and which was on screen — and the
/// only difference is that one was asked for. Two codecs for one list would be
/// two places for the escaping to drift, and this module's own doc records that
/// trap costing two directories of a Windows path twice already.
///
/// It is a distinct header because the two files mean different things to the
/// program: a session is claimed, consumed and deleted, and a project is opened
/// as many times as the user likes and never touched.
const PROJECT_HEADER: &str = "polylinker-project 1";
const SEPARATOR: &str = "--";

/// What was on the bench when this was written.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Session {
    /// The files that were open, in tab order.
    pub open: Vec<PathBuf>,
    /// How many open documents could not be listed, because they had never been
    /// written to a file.
    ///
    /// Reported, not hidden. "Reopened 4 documents" when there were six is a
    /// sentence that is true and misleading at once, and the two the user is
    /// missing are precisely the ones that only ever existed inside Polylinker.
    pub withheld: usize,
    /// Which of `open` was on screen.
    pub active: Option<usize>,
    /// When this was written, in seconds since the Unix epoch.
    pub saved_at: u64,
    /// The name the user gave this, if it was saved as a project.
    ///
    /// `None` for the automatic session file, which nobody named and nobody
    /// asked for.
    pub name: Option<String>,
    /// The window that wrote this has since closed.
    ///
    /// Without it, freshness alone locks a user out of their own bench: quit and
    /// relaunch inside the live window and the list is still "maybe live", so
    /// nothing is restored — the one moment a person is most certain their tabs
    /// should come back.
    ///
    /// The same reasoning `recover::Snapshot::abandoned` records applies, and so
    /// does its limit: this is written on the way OUT, with nothing crashing. A
    /// list that lacks it because the window died is correctly read as one that
    /// may still have an owner, and ages out of that ninety seconds later.
    pub closed: bool,
}

/// Encode a session. One `key: value` header, then one path per line.
pub fn encode(s: &Session) -> String {
    let mut out = String::with_capacity(64 + s.open.len() * 64);
    out.push_str(if s.name.is_some() {
        PROJECT_HEADER
    } else {
        HEADER
    });
    out.push('\n');
    if let Some(n) = &s.name {
        out.push_str(&format!("name: {}\n", escape(n)));
    }
    out.push_str(&format!("saved: {}\n", s.saved_at));
    out.push_str(&format!("withheld: {}\n", s.withheld));
    if let Some(i) = s.active {
        out.push_str(&format!("active: {i}\n"));
    }
    if s.closed {
        out.push_str("exit: closed\n");
    }
    out.push_str(SEPARATOR);
    out.push('\n');
    for p in &s.open {
        out.push_str(&escape(&p.display().to_string()));
        out.push('\n');
    }
    out
}

/// Decode a session, or nothing at all.
///
/// `None` rather than an error, and that asymmetry with `recover::decode` is the
/// point: a damaged recovery file still holds a molecule worth salvaging, and a
/// damaged tab list holds nothing anyone would miss.
pub fn decode(text: &str) -> Option<Session> {
    let mut lines = text.lines();
    let head = lines.next();
    if head != Some(HEADER) && head != Some(PROJECT_HEADER) {
        return None;
    }
    let mut s = Session::default();
    let mut sawsep = false;
    for line in lines.by_ref() {
        if line == SEPARATOR {
            sawsep = true;
            break;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "saved" => s.saved_at = v.parse().unwrap_or(0),
            "withheld" => s.withheld = v.parse().unwrap_or(0),
            "active" => s.active = v.parse().ok(),
            "exit" => s.closed = v == "closed",
            "name" => s.name = Some(unescape(v)),
            _ => {}
        }
    }
    if !sawsep {
        return None;
    }
    // THE FILE MUST END WHERE A LINE ENDS, and the header check above is not
    // enough to know that. `sawsep` is set at the FIRST separator, so a list
    // torn anywhere after it still parses: six tabs cut mid-path decode as
    // three real ones plus a stump of the fourth, `active` is dropped by the
    // bounds guard below, and the restore reports "1 no longer exist" naming a
    // path that never existed. [`encode`] always ends a line — `--\n` at the
    // very least — so a text that does not is a write that did not finish.
    //
    // What this does NOT catch, said plainly: a tear that lands exactly on a
    // newline is a shorter list that is indistinguishable from a shorter list,
    // and nothing short of a count or an atomic write can tell them apart. It
    // costs tabs rather than inventing a path, which is the lesser of the two.
    if !text.ends_with('\n') {
        return None;
    }
    s.open = lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| PathBuf::from(unescape(l)))
        .collect();
    // An index that names no tab is dropped rather than clamped. Clamping would
    // silently put the wrong document on screen, and the honest fallback — the
    // first tab — is what `None` already means.
    if s.active.is_some_and(|i| i >= s.open.len()) {
        s.active = None;
    }
    Some(s)
}

/// This process's session file.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(format!("session-{}", std::process::id()))
}

/// Has this run written its own list yet?
///
/// The other half of [`claim`]'s existence rule, and the reason it can afford
/// one: `claim` runs once, in `App::new`, BEFORE anything writes, so a
/// `session-{pid}` sitting there at that moment cannot be ours. This flag is
/// what makes that an assertion rather than a hope — once this process has
/// touched its own name, `claim` refuses it again, so a window can never
/// restore the bench it is sitting in even if some later caller asks it to.
///
/// Set BEFORE the write rather than after. `fs::write` truncates on open, so a
/// write that fails may still have replaced what was there; the direction that
/// costs nothing is to stop calling a name ours-once-touched a leftover.
static OURS_WRITTEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Write the list. Failure is silent, for the reason the module doc gives.
///
/// Not atomic, and the argument for that is narrower than it used to read. It
/// said "a truncated tab list is read by `decode` as no list at all", which was
/// false for the truncation that matters: `decode` accepted any prefix that
/// reached the `--`, so a torn write became a real, shorter bench. [`decode`]
/// now requires the text to end where a line ends, which is what makes the
/// sentence true — see the note there for the one tear that check cannot see.
/// With that in place the careful version buys only the line-boundary case, and
/// an `fsync` per tab switch is real.
pub fn write(path: &Path, s: &Session) -> Result<(), String> {
    if path.file_name().is_some_and(|n| {
        n == std::ffi::OsStr::new(format!("session-{}", std::process::id()).as_str())
    }) {
        OURS_WRITTEN.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    std::fs::write(path, encode(s)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Take the newest tab list left by another run, and clear the rest away.
///
/// `now` is passed in rather than read here so the liveness rule can be tested
/// without sleeping.
///
/// Anything a window may still be using is left strictly alone — not claimed,
/// not deleted, not counted. Everything older than the newest claimable one is
/// deleted, because it is a superseded list of tabs and keeping it would mean
/// the launch after next restores a bench from two sessions ago.
pub fn claim(dir: &Path, now: u64) -> Option<Session> {
    let mine = path(dir);
    // EXISTENCE, NOT THE NAME — `recover::claim`'s rule, and the same hazard.
    //
    // `std::process::id()` names a RUN only while that run is alive. Skipping
    // `session-{pid}` because it carries our number meant that when the OS
    // reissued a crashed run's PID, its whole bench was first ignored here and
    // then written over by `record_session` on the very next frame: six tabs
    // gone, silently, with no banner and no second chance. So a list at our own
    // name is a leftover unless THIS run put it there, which [`OURS_WRITTEN`]
    // is what knows.
    let ours_is_a_leftover = !OURS_WRITTEN.load(std::sync::atomic::Ordering::Relaxed);
    let mut found: Vec<(u64, PathBuf)> = Vec::new();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        // `session-` and not `session`, so a `.plproj` the user parked here is
        // never eaten. The claiming marker below is `session-{pid}.claiming`
        // (`with_extension` on a name that has no extension APPENDS), so it
        // does match this prefix and is skipped further down instead: it is
        // empty, and `decode("")` is `None`. That is incidental rather than
        // deliberate, and the marker being empty is what keeps it true.
        if !name.starts_with("session-") {
            continue;
        }
        let is_mine = p == mine;
        if is_mine && !ours_is_a_leftover {
            continue;
        }
        let Some(s) = std::fs::read_to_string(&p).ok().as_deref().and_then(decode) else {
            // Unreadable or not a session file. Left where it is: this function
            // deletes lists it has superseded, and something it cannot parse is
            // not something it can know it has superseded.
            continue;
        };
        // Fresh AND still open. A list its window has said goodbye to is this
        // launch's to take however recently it was written — which is the
        // ordinary case, since quitting and relaunching is how a person moves
        // their bench from one run to the next.
        //
        // Freshness does not apply to our own name at all. It is a guess about
        // whether some other process is still running, and no other process can
        // be holding the PID this one is holding — so a file there is a dead
        // run's however recently it was stamped, and asking "is it fresh?"
        // would put exactly the just-crashed bench back into the case that
        // loses it.
        if !is_mine && !s.closed && crate::recover::maybe_live(s.saved_at, now) {
            continue;
        }
        found.push((s.saved_at, p));
    }
    found.sort_by_key(|(t, p)| (std::cmp::Reverse(*t), p.clone()));
    let mut it = found.into_iter();
    let (_, newest) = it.next()?;
    let superseded: Vec<PathBuf> = it.map(|(_, p)| p).collect();
    // EXCLUSIVE BY CREATION, and see the module doc for what this replaced.
    //
    // `create_new` is `O_EXCL` / `CREATE_NEW`: the filesystem itself refuses the
    // second caller, and that refusal is the decision. It is the one primitive
    // here that is exclusive on both platforms — a rename is not, measured, on
    // this one.
    //
    // The marker is named after the LIST rather than after us, because what is
    // being claimed is the list. A crash between taking it and dropping it
    // leaves it behind, and the cost of that is bounded to one pid's worth of
    // names: the list it guards is `session-{pid}`, so the next run with a
    // different number is unaffected, and the same number is the case
    // `recover::claim` already treats as a leftover.
    //
    // NOTHING IS DELETED BEFORE THIS SUCCEEDS. Clearing the superseded lists
    // above the marker meant a launch that lost the claim — or found a marker a
    // crash had left behind — had already thrown away every bench but the one
    // it then failed to take, so a stall that is supposed to cost one launch
    // cost the older bench outright.
    let marker = newest.with_extension("claiming");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .ok()?;
    let out = std::fs::read_to_string(&newest)
        .ok()
        .as_deref()
        .and_then(decode);
    // Everything older than the one being claimed goes, because it is a
    // superseded list of tabs and keeping it would mean the launch after next
    // restores a bench from two sessions ago.
    for old in superseded {
        let _ = std::fs::remove_file(old);
    }
    // The list before the marker: a crash between these two leaves a marker with
    // nothing to guard, which is inert, where the other order leaves a list
    // nothing is guarding, which is a bench two windows could restore.
    let _ = std::fs::remove_file(&newest);
    let _ = std::fs::remove_file(&marker);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pl-session-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a temp directory");
        d
    }

    fn sample() -> Session {
        Session {
            open: vec![
                PathBuf::from(r"C:\Users\lior\OneDrive - Bar-Ilan\plasmids\pET28a.dna"),
                PathBuf::from("/home/lior/pUC19.gb"),
            ],
            withheld: 2,
            active: Some(1),
            saved_at: 1_785_000_000,
            name: None,
            closed: false,
        }
    }

    #[test]
    fn a_session_round_trips_including_the_paths_backslashes() {
        // The escape is the reason this is not a formality: a Windows path is
        // mostly backslashes, and a codec that eats one silently reopens a file
        // from a directory the user does not have.
        let s = sample();
        assert_eq!(decode(&encode(&s)), Some(s));
    }

    #[test]
    fn an_active_index_that_names_no_tab_is_dropped_rather_than_clamped() {
        let text = encode(&Session {
            active: Some(9),
            ..sample()
        });
        assert_eq!(decode(&text).unwrap().active, None);
    }

    #[test]
    fn anything_that_is_not_a_session_file_is_no_session_at_all() {
        assert_eq!(decode(""), None);
        assert_eq!(decode("LOCUS x\n"), None);
        // A header with no separator is a truncated write, which is exactly what
        // a non-atomic write can leave. It must read as no list, never as an
        // empty bench that would then be restored over the user's tabs.
        assert_eq!(decode(&format!("{HEADER}\nwithheld: 0\n")), None);
    }

    /// A project is the same list under a name, and must not be mistaken for a
    /// session.
    ///
    /// PROVEN TO FAIL against 806478a, where there is no `name` at all. The two
    /// files hold the same thing and mean different things: a session is
    /// claimed, consumed and deleted on the next launch, and a project is opened
    /// as many times as the user likes and never touched. A project picked up by
    /// `claim` would be silently eaten the first time Polylinker started.
    #[test]
    fn a_named_project_round_trips_and_is_never_claimed_as_a_session() {
        let named = Session {
            name: Some("June cloning".into()),
            ..sample()
        };
        let back = decode(&encode(&named)).expect("a project");
        assert_eq!(back, named, "the name did not survive the round trip");
        assert!(
            encode(&named).starts_with("polylinker-project"),
            "a project is written under the session header"
        );
        assert!(
            encode(&sample()).starts_with("polylinker-session"),
            "a session is written under the project header"
        );

        // And a project sitting in the state directory is not a bench to claim.
        // `claim` looks for `session-*`; a `.plproj` lives wherever the user put
        // it and is never named that, so the guard is the NAME rather than the
        // header — asserted here so that a future `claim` which scanned more
        // widely would have to face this.
        let d = dir("project");
        write(&d.join("June.plproj"), &named).unwrap();
        assert_eq!(
            claim(&d, 9_000_000),
            None,
            "a saved project was consumed as if it were an abandoned session"
        );
        assert!(d.join("June.plproj").exists(), "and it must still be there");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The rule that stops a second window stealing the first one's bench.
    #[test]
    fn a_list_another_window_is_still_writing_is_left_where_it_is() {
        let d = dir("live");
        let live = d.join("session-999998");
        write(
            &live,
            &Session {
                saved_at: 1_000_000,
                ..sample()
            },
        )
        .unwrap();
        assert_eq!(
            claim(&d, 1_000_010),
            None,
            "a second window claimed a running window's bench"
        );
        assert!(live.exists(), "and it must still be there for its owner");

        // Once it ages out it is the next launch's to take.
        assert!(claim(&d, 1_000_000 + 3_600).is_some());
        assert!(
            !live.exists(),
            "a claimed list is not left to be claimed twice"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Freshness alone would lock a user out of their own bench.
    ///
    /// Quit and relaunch — which is how a person moves a bench from one run to
    /// the next — and the list is seconds old, so a rule that only asked "is
    /// this fresh?" would refuse to restore at exactly the moment somebody is
    /// most certain their tabs should come back.
    #[test]
    fn a_bench_its_window_said_goodbye_to_is_restored_however_recent_it_is() {
        let d = dir("goodbye");
        write(
            &d.join("session-999996"),
            &Session {
                saved_at: 1_000_000,
                closed: true,
                ..sample()
            },
        )
        .unwrap();
        assert!(
            claim(&d, 1_000_001).is_some(),
            "relaunching one second after quitting lost the bench"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Two windows starting together must not both restore the same bench.
    ///
    /// THE SEQUENTIAL TEST BELOW DOES NOT PROVE THIS. Read-then-delete passes it
    /// exactly as rename does — the second call finds the file gone either way —
    /// so the property that matters, that no two callers can be inside the read
    /// at once, has no subject at all without concurrent callers.
    ///
    /// It can only fail in the safe direction: correct code cannot produce two
    /// winners, so a failure is always real and a pass merely fails to catch.
    /// Twenty rounds of eight threads, because a race caught one time in ten is
    /// a test that ships the bug nineteen times out of twenty.
    #[test]
    fn only_one_of_eight_windows_starting_at_once_gets_the_bench() {
        let d = dir("race");
        for round in 0..20 {
            let f = d.join(format!("session-99900{}", round % 10));
            write(
                &f,
                &Session {
                    saved_at: 1_000,
                    closed: true,
                    ..sample()
                },
            )
            .unwrap();
            let winners: usize = std::thread::scope(|s| {
                let hands: Vec<_> = (0..8)
                    .map(|_| s.spawn(|| claim(&d, 9_000_000).is_some()))
                    .collect();
                hands
                    .into_iter()
                    .map(|h| h.join().unwrap_or(false))
                    .filter(|won| *won)
                    .count()
            });
            assert_eq!(
                winners, 1,
                "round {round}: {winners} windows restored the same bench, so there are \
                 {winners} copies of every tab and no telling which one was edited"
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Claiming consumes the list, so the next launch does not restore a bench
    /// that is already on somebody's screen.
    #[test]
    fn one_list_is_restored_once_and_not_by_everyone_who_looks() {
        let d = dir("once");
        write(
            &d.join("session-999997"),
            &Session {
                saved_at: 1_000,
                ..sample()
            },
        )
        .unwrap();
        assert!(claim(&d, 9_000_000).is_some(), "the premise");
        assert_eq!(
            claim(&d, 9_000_000),
            None,
            "the same bench was handed out twice"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A list torn mid-path is no list, not a shorter one.
    ///
    /// PROVEN TO FAIL without the newline requirement in `decode`: the six-tab
    /// list below comes back as `["a.gb", "b.gb", "c.gb", "d"]` — three real
    /// tabs and a stump of a fourth — with `active` silently dropped by the
    /// bounds guard. The restore then says "reopened 3 document(s), 1 no longer
    /// exists" and names `d`, a file the user never had, while `claim` has
    /// already deleted the only copy of the real list.
    #[test]
    fn a_list_torn_mid_path_is_no_list_rather_than_a_shorter_one() {
        let six = Session {
            open: ["a.gb", "b.gb", "c.gb", "d.gb", "e.gb", "f.gb"]
                .iter()
                .map(PathBuf::from)
                .collect(),
            active: Some(5),
            ..sample()
        };
        let text = encode(&six);
        assert_eq!(decode(&text).as_ref(), Some(&six), "the premise");
        // One byte into the fourth path, which is what a write that stopped
        // part way through leaves — the separator and three whole tabs are
        // already on disk.
        let cut = text.find("d.gb").expect("the fourth path") + 1;
        assert_eq!(
            decode(&text[..cut]),
            None,
            "a torn write was read as a real, shorter bench"
        );
        // And every whole prefix is refused too, up to the one that is the
        // whole file: the property is "it ends where a line ends", not "it is
        // long enough".
        assert_eq!(decode(&text[..text.len() - 1]), None);
    }

    /// The crashed run's bench is at OUR name, and skipping it is how it dies.
    ///
    /// PROVEN TO FAIL with `|| p == mine` back in the filter: the leftover is
    /// not claimed, and `record_session` — which runs on the first frame — then
    /// writes this run's empty bench straight over it.
    ///
    /// BOTH HALVES IN ONE TEST ON PURPOSE. `OURS_WRITTEN` is process-global, as
    /// it must be, so the half that sets it has to run after the half that
    /// needs it clear; two `#[test]`s would be two threads in an order nobody
    /// chooses.
    #[test]
    fn a_bench_left_at_our_own_pid_is_a_leftover_until_this_run_writes_there() {
        let d = dir("pidreuse");
        let leftover = path(&d);
        // Written raw rather than through `write`: a DIFFERENT process left
        // this, the one that held this PID before the OS reissued it, so this
        // run must not be recorded as having written its own list.
        std::fs::write(
            &leftover,
            encode(&Session {
                saved_at: 1_000_000,
                ..sample()
            }),
        )
        .unwrap();
        // Fresh and not `closed` — what a crash leaves — and claimed anyway,
        // because no other process can be holding the PID this one holds.
        let got = claim(&d, 1_000_005).expect("the crashed run's bench was ignored, then lost");
        assert_eq!(got.open, sample().open);
        assert!(
            !leftover.exists(),
            "a claimed list is not left to be claimed twice"
        );

        // And once this run has written there, that same name is ours: a window
        // must never restore the bench it is sitting in.
        let d2 = dir("ours");
        let ours = path(&d2);
        write(
            &ours,
            &Session {
                saved_at: 1_000_000,
                closed: true,
                ..sample()
            },
        )
        .unwrap();
        assert_eq!(
            claim(&d2, 9_000_000),
            None,
            "a window claimed the bench it is sitting in"
        );
        assert!(ours.exists(), "and did not consume it either");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&d2);
    }

    /// A leftover marker stalls one launch. It must not also cost a bench.
    ///
    /// PROVEN TO FAIL with the `remove_file` loop back above the marker: the
    /// older list is deleted by a launch that goes on to restore nothing, so
    /// the one bench that was still recoverable is gone.
    #[test]
    fn a_marker_left_behind_stalls_the_claim_without_destroying_what_it_stalls() {
        let d = dir("marker");
        for (pid, at, file) in [("999988", 1_000u64, "old.gb"), ("999989", 5_000, "new.gb")] {
            write(
                &d.join(format!("session-{pid}")),
                &Session {
                    open: vec![PathBuf::from(file)],
                    saved_at: at,
                    closed: true,
                    ..sample()
                },
            )
            .unwrap();
        }
        // The name is the one the comment in `claim` gives, asserted rather
        // than described: `with_extension` on a name with no extension appends.
        let marker = d.join("session-999989").with_extension("claiming");
        assert_eq!(
            marker.file_name().unwrap().to_string_lossy(),
            "session-999989.claiming"
        );
        // Left by a run that crashed between taking it and dropping it.
        std::fs::write(&marker, "").unwrap();

        assert_eq!(claim(&d, 9_000_000), None, "the marker did not hold");
        assert!(
            d.join("session-999988").exists(),
            "a launch that restored nothing deleted the bench it could still have restored"
        );
        assert!(
            d.join("session-999989").exists(),
            "nor the one it stalled on"
        );

        // And when the marker goes, the bench the stall was protecting is still
        // there to be restored — after which the older one is cleared as
        // superseded, which is a claim that succeeded rather than one that did
        // not.
        std::fs::remove_file(&marker).unwrap();
        assert_eq!(
            claim(&d, 9_000_000).expect("a list").open,
            vec![PathBuf::from("new.gb")]
        );
        assert!(!d.join("session-999988").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The newest wins and the ones it supersedes go, or the launch after next
    /// restores a bench from two sessions ago.
    #[test]
    fn the_newest_list_wins_and_the_ones_behind_it_are_cleared_away() {
        let d = dir("newest");
        for (pid, at, file) in [
            ("999990", 1_000u64, "old.gb"),
            ("999991", 5_000, "new.gb"),
            ("999992", 3_000, "middling.gb"),
        ] {
            write(
                &d.join(format!("session-{pid}")),
                &Session {
                    open: vec![PathBuf::from(file)],
                    withheld: 0,
                    active: None,
                    saved_at: at,
                    name: None,
                    closed: true,
                },
            )
            .unwrap();
        }
        let got = claim(&d, 9_000_000).expect("a list");
        assert_eq!(got.open, vec![PathBuf::from("new.gb")]);
        assert_eq!(
            claim(&d, 9_000_000),
            None,
            "a superseded list survived and will be restored next time"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

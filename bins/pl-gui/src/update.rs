//! The once-per-run update check: whether to ask, asking off the UI thread, and
//! what to say about the answer.
//!
//! # This module is the whole network surface of the desktop app
//!
//! Nothing else in `bins/pl-gui` calls `pl_update`, and
//! `only_the_update_module_can_reach_the_network` in `main.rs`'s test module
//! reads the sources to keep it that way. The crate is spelled `pl_update::` at
//! each use site rather than imported, for the reason `bins/pl/src/main.rs`
//! gives: an import is how a network surface stops being greppable.
//!
//! # Why it is off until somebody turns it on
//!
//! Polylinker's claim is that it sends nothing anywhere, and an update check is
//! a beacon: it tells whoever answers on that hostname that this machine exists
//! and is running this version. For a user holding unpublished sequence on a
//! bench machine that is a real cost, and it is not one anybody should pay by
//! default because a developer found it convenient. So [`Settings::update_check`]
//! ships `false`, the checkbox that changes it states what gets sent in the
//! sentence next to it, and a damaged settings file falls back to off
//! (`settings.rs`).
//!
//! # "At most once per run", and what enforces it
//!
//! [`Check::started`] is a latch. It is set the first time a check begins and
//! never cleared, so:
//!
//! * the check cannot repeat because the window was resized, refocused, or
//!   redrawn — and a GUI redraws constantly, which is what makes a naive "check
//!   if we have not got a result yet" into an accidental request loop;
//! * turning the setting off and on again inside one run does not buy a second
//!   check;
//! * a failed check is not retried. That is deliberate. A retry is the first
//!   step towards a poll, and a user who wants another answer has `pl update
//!   --check`, which is a thing they typed.
//!
//! There is no timer here, no clock, and nothing stored on disk about when a
//! check last happened. The only way to get a second check is to start the
//! program again.
//!
//! # Why the GUI does not download
//!
//! `docs/RELEASING.md`'s requirement 4 is that a running binary is never
//! replaced silently, and `pl_update` keeps it by handing back a path and
//! stopping. A GUI could do the same — but the honest version of "download 60 MB
//! in the background and then show a path" needs progress, cancellation, and a
//! story for what happens when the window closes mid-transfer, and every one of
//! those is a place for the promise to erode into "and then we run it for you".
//! The notice names `pl update` instead. That is a worse click-count and a
//! better guarantee, and the task that asked for this said a notice plus the
//! command was an acceptable and honest outcome.

use std::sync::mpsc::{channel, Receiver};

/// What the release page said, if it was asked.
#[derive(Default)]
pub enum State {
    /// Not asked. Every run starts here, including one whose setting is on —
    /// the check is started from the first frame, not from `Default`.
    #[default]
    Idle,
    /// Asked, waiting. The worker owns the other end.
    Waiting(Receiver<Result<pl_update::Check, String>>),
    /// Answered: nothing newer is offered.
    Current,
    /// Answered: something newer is offered. The string is a `Version`
    /// rendered, not a version the caller may build a URL from — the GUI does
    /// not fetch, so it has no use for the typed form.
    Newer(String),
    /// It did not work, and the reason is kept.
    ///
    /// Shown rather than swallowed. A check that fails silently is
    /// indistinguishable from a check that keeps saying "you are up to date",
    /// and of the two possible mistakes — bothering somebody with "could not
    /// reach github.com", or letting them believe they are current when nothing
    /// has been asked in six months — the second is the one that matters.
    Failed(String),
}

/// The check, and the latch that stops it happening twice.
#[derive(Default)]
pub struct Check {
    pub state: State,
    /// Set when a check starts, never cleared. See the module doc.
    started: bool,
    /// Set when the result has been announced once, so the status line is
    /// written on the frame the answer arrives and not on every frame after it.
    announced: bool,
}

/// The page a person is sent to. The same constant the updater builds its URLs
/// from, so the button and the download cannot point at different projects.
pub const RELEASE_PAGE: &str = pl_update::RELEASE_BASE_URL;

/// Exactly what the request discloses, in one sentence, for the checkbox.
///
/// A constant rather than a literal at the call site because it is also what
/// the test asserts on: the setting may not exist without the sentence, and
/// `the_update_setting_says_what_it_sends` is what makes that true rather than
/// customary.
pub const WHAT_IS_SENT: &str = "Once per launch, Polylinker asks github.com whether a newer \
     release exists. It sends no sequence, no file name and no identifier — the request tells \
     that server your IP address and nothing about your work. It never downloads or installs \
     anything; to do that you run `pl update` yourself. Off unless you switch it on.";

impl Check {
    /// May a check start now, and if so, latch it so none ever starts again?
    ///
    /// Split out from [`Check::maybe_start`] deliberately, and the reason is
    /// testability of the only rule in this file that matters. "At most once per
    /// run" is a claim about a state transition, and a test that established it
    /// by calling `maybe_start` would spawn a worker that runs `curl` — a test
    /// suite that contacts github.com, fails on a CI leg with no egress, and is
    /// the one part of this repository that phones home. Written this way the
    /// rule is exhaustively testable with no I/O at all, and what is left
    /// untested is the four lines that hand a URL to a thread.
    ///
    /// It returns `bool` rather than taking a closure so that the latch is set
    /// in the same expression that grants permission: there is no ordering in
    /// which a caller can be told "yes" and leave `started` false.
    fn claim(&mut self, enabled: bool) -> bool {
        if self.started || !enabled {
            return false;
        }
        self.started = true;
        true
    }

    /// Start the check, if it is switched on and has not been started already.
    ///
    /// Called every frame. Doing nothing is the overwhelmingly common outcome
    /// and has to be cheap: two bools, no allocation, no clock.
    pub fn maybe_start(&mut self, enabled: bool) {
        if !self.claim(enabled) {
            return;
        }
        let (tx, rx) = channel();
        // A worker, because `pl_update::check` runs `curl` and waits for it —
        // up to 15 s to connect and 30 s in total (`pl_update::net`). On the UI
        // thread that is a frozen window, and a tool that hangs for half a
        // minute at launch because it is "checking for updates" is precisely the
        // behaviour this setting is off by default to avoid inflicting.
        //
        // The thread outlives nothing: it makes one request, sends one message,
        // and ends. If the window closed first the send fails, which is fine and
        // is the same contract `library.rs` and `doc.rs` already use.
        std::thread::Builder::new()
            .name("pl-update-check".into())
            .spawn(move || {
                let answer =
                    pl_update::check(&pl_update::Curl::default()).map_err(|e| e.to_string());
                let _ = tx.send(answer);
            })
            // A machine that cannot spawn a thread is in no state to be told
            // about a new release, and this must not be the thing that takes the
            // window down. `started` is already set, so a failure here means no
            // check this run rather than an attempt on every frame.
            .map(|_| self.state = State::Waiting(rx))
            .unwrap_or_else(|e| self.state = State::Failed(e.to_string()));
    }

    /// Collect the worker's answer if it has arrived.
    ///
    /// Returns the line to put on the status bar, once, on the frame the result
    /// lands — and `None` on every other frame, which is what stops a notice
    /// from overwriting whatever the user is doing for the rest of the session.
    pub fn poll(&mut self) -> Option<String> {
        if let State::Waiting(rx) = &self.state {
            if let Ok(answer) = rx.try_recv() {
                self.state = match answer {
                    Ok(c) if c.update_available() => State::Newer(c.offered.to_string()),
                    Ok(_) => State::Current,
                    Err(e) => State::Failed(e),
                };
            }
        }
        if self.announced {
            return None;
        }
        let said = match &self.state {
            State::Newer(v) => format!("Polylinker {v} has been released — see Help"),
            // Deliberately silent about the two boring outcomes. "You are up to
            // date" is a notification about nothing happening, and the Help menu
            // holds the answer for anyone who goes looking. A failure is not
            // pushed into the user's way either: they did not ask for this at
            // the moment it ran, and it changes nothing about their document.
            State::Current | State::Failed(_) => {
                self.announced = true;
                return None;
            }
            State::Idle | State::Waiting(_) => return None,
        };
        self.announced = true;
        Some(said)
    }

    /// One line for the Help menu, always available once a check has happened.
    pub fn summary(&self) -> Option<String> {
        match &self.state {
            State::Idle => None,
            State::Waiting(_) => Some("Checking for a newer release…".to_string()),
            State::Current => Some("This is the current release.".to_string()),
            State::Newer(v) => Some(format!("Polylinker {v} is available.")),
            State::Failed(e) => Some(format!("Could not check: {e}")),
        }
    }

    /// Is there something newer to point at? Decides whether the Help menu
    /// offers the release-page button.
    pub fn offers_newer(&self) -> bool {
        matches!(self.state, State::Newer(_))
    }

    /// Is a worker still out there? The frame loop keeps repainting while this
    /// is true, because a worker thread cannot wake the UI by itself — the same
    /// contract `library.rs` and `doc.rs` already run on.
    pub fn is_waiting(&self) -> bool {
        matches!(self.state, State::Waiting(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every line in the desktop app that can open a socket is in this file.
    ///
    /// PROVEN TO FAIL by adding a `pl_update::curl_available()` call to
    /// `App::pick_file` in `main.rs`: red here, green in all 500-odd other
    /// tests in this binary, because a file picker with a network probe bolted
    /// onto it still picks files. That is the whole reason this reads the
    /// sources instead of driving the UI, and it is the same shape as
    /// `crates/pl-update/tests/handoff.rs` and
    /// `crates/pl-design/tests/purity.rs`.
    ///
    /// **It lives in the module it is about, and that is not laziness.** This
    /// file is the one the scan skips, so the test can name the crate it is
    /// looking for without matching itself — which is exactly what happened
    /// when it was first written in `main.rs`, where it failed on its own
    /// source. The alternative was assembling the needle at run time to hide it
    /// from the scan, which would make the test's own text a puzzle.
    ///
    /// The rule it enforces is also a style rule, deliberately: the crate is
    /// spelled `pl_update::` at each use site, so `grep -rn pl_update
    /// bins/pl-gui/src` returns the app's entire network surface. A `use
    /// pl_update::Check;` at the top of another module would put the name
    /// outside this file and fail here, which is the intended answer.
    #[test]
    fn only_the_update_module_can_reach_the_network() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut checked = 0;
        let mut offences = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("bins/pl-gui/src") {
            let path = entry.expect("a readable entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a utf-8 file name")
                .to_string();
            if name == "update.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable");
            checked += 1;
            for (i, line) in text.lines().enumerate() {
                // Prose has to be able to name the crate in order to say where
                // it lives -- `main.rs`'s `mod update;` doc comment does. Doc
                // comments and ordinary comments both start `//`.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if line.contains("pl_update") {
                    offences.push(format!("{name}:{}: {}", i + 1, line.trim()));
                }
            }
        }
        assert!(
            checked >= 15,
            "only {checked} source file(s) were scanned; this test parsed almost nothing"
        );
        assert!(
            offences.is_empty(),
            "the desktop app may reach the network only from update.rs; these are \
             outside it:\n  {}\nIf one is a `use`, spell the call at its site instead \
             -- an import is how this stops being greppable.",
            offences.join("\n  ")
        );

        // Not vacuous: this module really does still call the crate. Deleting
        // the check entirely would satisfy every assertion above.
        let module = std::fs::read_to_string(dir.join("update.rs")).expect("update.rs");
        for call in ["pl_update::check(", "pl_update::Curl"] {
            assert!(module.contains(call), "update.rs no longer calls {call}");
        }
        // And it must NOT have grown the download. The GUI shows a notice and
        // names `pl update`; the fetch here would be the first half of an
        // installer, which is what requirement 4 of docs/RELEASING.md is written
        // against. See this module's doc for the argument.
        //
        // ASSEMBLED, not written out, and this one has no way round it: the
        // assertion is that a string is ABSENT from a file, and this is that
        // file. Spelled literally it would match itself and fail on the very
        // source it is defending -- which it did, on the first run. The two
        // positive checks above are safe written out precisely because they are
        // positive: they are supposed to find something.
        let downloader = format!("pl_update::{}", "fetch_and_verify");
        assert!(
            !module.contains(&downloader),
            "the desktop app has grown a downloader ({downloader}); see this module's \
             doc for why the notice names `pl update` instead"
        );
    }

    /// A switched-off check starts nothing, however many frames go by.
    ///
    /// This is the first-run promise in executable form. `maybe_start` is called
    /// from the paint loop, so "off means off" has to survive being asked
    /// hundreds of times, which is exactly the shape of mistake a `!self.started`
    /// check written the other way round would make.
    #[test]
    fn a_switched_off_check_never_starts_however_many_frames_pass() {
        let mut c = Check::default();
        for _ in 0..500 {
            c.maybe_start(false);
            assert!(matches!(c.state, State::Idle));
            assert!(c.summary().is_none(), "an unasked check must claim nothing");
            assert!(!c.started, "a switched-off run must not latch the check");
        }
    }

    /// Switched on, permission is granted exactly once and never again.
    ///
    /// `claim` and not `maybe_start`, for the reason `claim`'s own doc gives: at
    /// `maybe_start` this assertion would cost a real request to github.com from
    /// `cargo test`. Every frame after the first must be refused, because
    /// `maybe_start` is called from the paint loop and "not yet answered,
    /// therefore ask" is a request per frame at sixty frames a second.
    #[test]
    fn a_switched_on_check_is_permitted_exactly_once() {
        let mut c = Check::default();
        assert!(c.claim(true), "the first frame must be allowed to ask");
        assert!(c.started);
        for frame in 0..1000 {
            assert!(
                !c.claim(true),
                "frame {frame} was allowed to ask a second time"
            );
        }
    }

    /// Turning the setting off and on again inside one run buys no second check.
    ///
    /// Somebody who unticks the box and reticks it is not asking for another
    /// request; they are looking at the sentence next to it. The latch is what
    /// makes that free.
    #[test]
    fn toggling_the_setting_does_not_buy_a_second_check() {
        let mut c = Check::default();
        assert!(c.claim(true));
        for _ in 0..100 {
            assert!(!c.claim(false));
            assert!(!c.claim(true), "a toggle bought a second check");
        }
    }

    /// A run that started switched off may still ask if the user switches it on.
    ///
    /// The control for the two tests above, and it is not a formality: a latch
    /// set on the first frame regardless of the setting would pass both of them
    /// and would make the checkbox do nothing until the next launch.
    #[test]
    fn switching_it_on_mid_run_is_what_the_checkbox_is_for() {
        let mut c = Check::default();
        for _ in 0..100 {
            assert!(!c.claim(false));
        }
        assert!(
            c.claim(true),
            "ticking the box did nothing, because an off frame had latched the check"
        );
    }

    /// The status line is written once, on the frame the answer lands.
    ///
    /// A notice that reappeared every frame would sit on top of every other
    /// message the app has to give — "wrote map.svg", "9 features dropped" — for
    /// the rest of the session.
    #[test]
    fn a_new_release_is_announced_once_and_then_stops() {
        let mut c = Check {
            state: State::Newer("9.9.9".into()),
            started: true,
            announced: false,
        };
        let first = c.poll().expect("the frame the answer lands says so");
        assert!(first.contains("9.9.9"), "{first}");
        for _ in 0..100 {
            assert!(c.poll().is_none(), "the notice repeated on a later frame");
        }
        // And the Help menu keeps the answer after the status line has moved on.
        assert!(c.summary().unwrap().contains("9.9.9"));
        assert!(c.offers_newer());
    }

    /// Being up to date is not news, and neither is a failed check.
    #[test]
    fn the_quiet_outcomes_never_reach_the_status_line() {
        for state in [State::Current, State::Failed("no route to host".into())] {
            let mut c = Check {
                state,
                started: true,
                announced: false,
            };
            for _ in 0..100 {
                assert!(c.poll().is_none(), "a quiet outcome interrupted the user");
            }
            // Quiet is not the same as hidden: Help still answers.
            assert!(c.summary().is_some());
            assert!(!c.offers_newer());
        }
    }

    /// The sentence beside the checkbox says what the request contains, and what
    /// it does not.
    ///
    /// Asserted here rather than left to review because it is the whole basis on
    /// which somebody consents. Each phrase is a specific claim that the code
    /// above has to keep: one request per launch, no payload, no download.
    #[test]
    fn the_update_setting_says_what_it_sends() {
        for required in [
            "github.com",
            "no sequence",
            "no file name",
            "IP address",
            "never downloads",
            "Off unless you switch it on",
        ] {
            assert!(
                WHAT_IS_SENT.contains(required),
                "the consent sentence no longer says {required:?}"
            );
        }
    }

    /// The button and the updater agree about which project this is.
    #[test]
    fn the_release_page_is_the_updaters_own_base_url() {
        assert_eq!(RELEASE_PAGE, pl_update::RELEASE_BASE_URL);
        assert!(RELEASE_PAGE.starts_with("https://"));
    }
}

//! The transport: `curl`, invoked as a program, with the flags argued for.
//!
//! # Why a subprocess and not an HTTP client
//!
//! Everything under `crates/` takes zero external dependencies, which leaves
//! three options for fetching bytes over TLS, and only one of them is
//! defensible.
//!
//! * **A crate.** `ureq`, `reqwest`, `rustls` — each pulls in dozens of
//!   transitive dependencies, and this is the one crate in the workspace whose
//!   whole job is to be harder to tamper with than the thing it protects.
//!   `Cargo.toml` sets out why even one dependency was an argument.
//! * **Hand-rolled TLS.** No. A verifier for a single signature algorithm is
//!   1,900 auditable lines (`pl_core::ed25519`); a TLS 1.3 stack with X.509
//!   path building and a trust store is not that, and getting it subtly wrong
//!   would be *worse* than not having it, because the padlock would still
//!   appear to be there.
//! * **The system `curl`.** Present in `%SystemRoot%\System32` on Windows 10
//!   and later, in `/usr/bin` on macOS, and in every mainstream Linux
//!   distribution; uses the operating system's own trust store, which is the
//!   one the user's administrator actually manages; and is patched by the
//!   system rather than by us.
//!
//! The third, then — with the understanding that the TLS is not what this
//! crate's guarantee rests on. Requirement 2 of `docs/RELEASING.md` exists
//! precisely because transport security says nothing about whoever is at the
//! other end: the Ed25519 signature over the manifest is the guarantee, and it
//! would still hold if this fetched over plain HTTP. TLS is defence in depth,
//! and the flags below are chosen so that it cannot be silently downgraded to
//! nothing.
//!
//! # The flags, each of which is there for a reason
//!
//! | Flag | Why |
//! |---|---|
//! | `--disable` | **First argument, and only effective there.** Stops `curl` reading `~/.curlrc` (`%APPDATA%\_curlrc` on Windows). Without it, a line in a file this program never looks at can add `--insecure`, a `--proxy`, or a `--capath` pointing anywhere, and every other flag here becomes advisory. |
//! | `--fail` | Without it `curl` exits 0 on a 404 and writes the error page to the output, so a missing release becomes a "manifest" of HTML. |
//! | `--silent --show-error` | No progress meter on a pipe, but errors still on stderr, where the error type can quote them. |
//! | `--location` | GitHub redirects release assets to `objects.githubusercontent.com`. Without this, every download is a 302 body. |
//! | `--proto =https` | The `=` makes it an absolute set, not an addition: **only** https, so a redirect chain cannot end at `file://`, `ftp://` or `scp://`. |
//! | `--proto-redir =https` | The one that is easy to forget. `--proto` constrains the URL given; this constrains where a redirect may go, and `--location` is on. |
//! | `--max-redirs` | A bounded chain. |
//! | `--tlsv1.2` | A floor, not a ceiling: 1.3 is still negotiated. |
//! | `--globoff` | `curl` otherwise treats `[`, `]` and `{}` in a URL as globbing syntax. These URLs contain none, and a URL builder is a bad place to rely on that staying true. |
//! | `--connect-timeout`, `--max-time` | A hung server must not hang the program that called this. |
//! | `--max-filesize` | A bound on what a hostile endpoint can make this write or hold. See [`Curl::get`] for what it does and does not enforce. |
//! | `--url` | The URL is the **value of an option**, so it can never be read as one, whatever it starts with. Belt and braces: the URLs in `flow.rs` are built from a compiled-in base and a [`crate::Version`], which cannot contain a `-`. |
//!
//! **`--insecure` appears nowhere and must never.** [`audit`] fails on it, and
//! on every other spelling of it, and is run against the real argv in the tests
//! below rather than being a rule in a comment.
//!
//! # No shell, ever
//!
//! [`std::process::Command`] does not invoke a shell: on Unix it `execve`s
//! directly, and on Windows it builds a command line for `CreateProcess`, which
//! `curl.exe` parses with the ordinary MSVC rules. Nothing here is ever handed
//! to `cmd.exe` or `sh -c`, so there is no metacharacter to escape and no
//! quoting bug to have. Note in passing that `curl` in a PowerShell prompt is
//! an alias for `Invoke-WebRequest` — that alias belongs to PowerShell's
//! parser, and a `CreateProcess` of `"curl"` resolves through `PATH` to
//! `System32\curl.exe`. What this crate runs and what a PowerShell user types
//! are not the same program.

use crate::error::UpdateError;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

/// Seconds allowed for the TCP and TLS handshake.
pub const CONNECT_TIMEOUT_SECS: u32 = 15;
/// Seconds allowed for a whole small-text fetch.
pub const TEXT_MAX_TIME_SECS: u32 = 30;
/// Seconds allowed for a whole artifact download. The archives are tens of
/// megabytes and a conference hotel is slow, so this is generous rather than
/// tight; it is a bound on hanging, not a service-level objective.
pub const ARTIFACT_MAX_TIME_SECS: u32 = 900;
/// The largest artifact this will accept. The 0.1.1 release's biggest asset is
/// a few tens of megabytes; half a gigabyte is far above anything this project
/// would publish and far below anything that would quietly fill a disk.
pub const ARTIFACT_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// Where `curl` should put what it fetched.
#[derive(Clone, Copy)]
enum Sink<'a> {
    /// Standard output, captured into memory by the caller.
    Memory { max_bytes: u64 },
    /// A file, named by this program and never by the server.
    File { path: &'a Path, max_bytes: u64 },
}

/// Fetching bytes from a URL.
///
/// A trait, and the only reason is testability — but it is a good reason. Every
/// refusal this crate has to prove (a bad signature, a flipped bit, a digest
/// that does not match, a manifest missing this platform) needs a server that
/// serves exactly those bytes, and a test that reached the real network would
/// be testing GitHub's availability rather than this code. The tests substitute
/// an in-memory implementation; the shipped path is [`Curl`].
///
/// It is deliberately two methods rather than one that returns bytes. An
/// artifact is tens of megabytes, and reading it through this process's memory
/// to write it out again would double the peak for nothing.
pub trait Fetch {
    /// Fetch a small resource into memory. `limit` is a hard ceiling; more than
    /// that is [`UpdateError::TooLarge`], never a truncation.
    fn get(&self, url: &str, limit: usize) -> Result<Vec<u8>, UpdateError>;

    /// Fetch a large resource straight to `to`, which must be a path this
    /// program chose.
    fn download(&self, url: &str, to: &Path) -> Result<(), UpdateError>;
}

/// The system `curl`.
#[derive(Debug, Clone, Copy)]
pub struct Curl {
    pub connect_timeout_secs: u32,
    pub text_max_time_secs: u32,
    pub artifact_max_time_secs: u32,
}

impl Default for Curl {
    fn default() -> Self {
        Curl {
            connect_timeout_secs: CONNECT_TIMEOUT_SECS,
            text_max_time_secs: TEXT_MAX_TIME_SECS,
            artifact_max_time_secs: ARTIFACT_MAX_TIME_SECS,
        }
    }
}

impl Curl {
    /// The argument vector, built and never interpolated.
    ///
    /// Split out from the call so the tests can read it. A test that could only
    /// observe the flags by watching a real network request would not be a test
    /// of the flags.
    fn argv(&self, url: &str, sink: Sink<'_>, max_time: u32) -> Vec<OsString> {
        let mut argv: Vec<OsString> = Vec::new();
        let mut flag = |s: &str| argv.push(OsString::from(s));

        // FIRST, and only meaningful first: ignore curlrc.
        flag("--disable");
        flag("--fail");
        flag("--silent");
        flag("--show-error");
        flag("--location");
        flag("--proto");
        flag("=https");
        flag("--proto-redir");
        flag("=https");
        flag("--max-redirs");
        flag("5");
        flag("--tlsv1.2");
        flag("--globoff");
        flag("--connect-timeout");
        flag(&self.connect_timeout_secs.to_string());
        flag("--max-time");
        flag(&max_time.to_string());

        match sink {
            Sink::Memory { max_bytes } => {
                flag("--max-filesize");
                flag(&max_bytes.to_string());
            }
            Sink::File { path, max_bytes } => {
                flag("--max-filesize");
                flag(&max_bytes.to_string());
                argv.push(OsString::from("--output"));
                argv.push(path.as_os_str().to_os_string());
            }
        }

        // The URL as an option's value, last.
        argv.push(OsString::from("--url"));
        argv.push(OsString::from(url));
        argv
    }

    /// Run `curl` with `argv` and hand back its captured output.
    ///
    /// The one place a missing `curl` is turned into
    /// [`UpdateError::CurlMissing`]: `Command::spawn` reports `NotFound` when
    /// the program cannot be resolved, which is a different condition from
    /// every other way this can fail and gets a different message. There is no
    /// separate probe run first, because a probe would be a second chance for
    /// the answer to be stale between the probe and the fetch.
    fn run(&self, argv: &[OsString], url: &str) -> Result<Vec<u8>, UpdateError> {
        // The rules in [`audit`] are checked here, on the real argument vector,
        // before the process exists — not only in the tests. The tests are what
        // stop the rules being vacuous; this is what stops a future edit that
        // slips past review from reaching the network at all. It costs one pass
        // over twenty short strings per request.
        if let Err(why) = audit(argv) {
            return Err(UpdateError::UnsafeRequest { why });
        }
        let mut cmd = Command::new(PROGRAM);
        cmd.args(argv);
        no_console(&mut cmd);
        let output = cmd.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                UpdateError::CurlMissing
            } else {
                UpdateError::Transport {
                    url: url.to_string(),
                    detail: format!("could not run curl: {e}"),
                }
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = match output.status.code() {
                Some(c) => format!("curl exited {c}"),
                None => "curl was killed by a signal".to_string(),
            };
            let detail = if stderr.trim().is_empty() {
                code
            } else {
                format!("{code}: {}", stderr.trim())
            };
            return Err(UpdateError::Transport {
                url: url.to_string(),
                detail,
            });
        }
        Ok(output.stdout)
    }
}

impl Fetch for Curl {
    /// # What `--max-filesize` does and does not do
    ///
    /// `curl` refuses a transfer whose size it is *told* in advance and which
    /// exceeds the limit. A server that sends no `Content-Length` — a chunked
    /// response, which a hostile one would choose — is not caught by it, and
    /// `curl`'s own documentation says so. That is why the length is checked
    /// again here after the fact, and why `--max-time` is set: the flag stops
    /// the honest case cheaply, the check stops the dishonest one, and the
    /// timeout bounds how long the dishonest one can go on for. The memory is
    /// already spent by the time the second check fires; bounding it properly
    /// would mean streaming, which means not using `Command::output`, which
    /// means a pipe-reading loop for a resource that is legitimately 400 bytes.
    fn get(&self, url: &str, limit: usize) -> Result<Vec<u8>, UpdateError> {
        let argv = self.argv(
            url,
            Sink::Memory {
                max_bytes: limit as u64,
            },
            self.text_max_time_secs,
        );
        let body = self.run(&argv, url)?;
        if body.len() > limit {
            return Err(UpdateError::TooLarge {
                url: url.to_string(),
                limit,
            });
        }
        Ok(body)
    }

    fn download(&self, url: &str, to: &Path) -> Result<(), UpdateError> {
        let argv = self.argv(
            url,
            Sink::File {
                path: to,
                max_bytes: ARTIFACT_MAX_BYTES,
            },
            self.artifact_max_time_secs,
        );
        self.run(&argv, url)?;
        // `--fail` plus a zero exit means curl wrote the body, but the file is
        // what the next step hashes, so its existence is confirmed here rather
        // than assumed from an exit code.
        if !to.exists() {
            return Err(UpdateError::Transport {
                url: url.to_string(),
                detail: "curl reported success but wrote no file".to_string(),
            });
        }
        Ok(())
    }
}

/// Every rule the argument vector must satisfy, in executable form.
///
/// This is the shape the house rule about checks that cannot fail demands.
/// Asserting `argv.contains("--fail")` in a test proves that one flag survived;
/// it says nothing about `--insecure` having been *added*, which is the edit
/// that would actually matter and which no positive assertion can see. So the
/// rules live in a function, the function is run against the real argv, and it
/// is also run against a dozen deliberately broken vectors in
/// [`tests::the_audit_rejects_the_argument_vectors_that_would_matter`] — which
/// is what stops it being a check that cannot fail.
pub(crate) fn audit(argv: &[OsString]) -> Result<(), String> {
    let as_str: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    // Anything that would weaken or redirect the transfer. `-k` is
    // `--insecure`; `--proto-default` would let a scheme-less URL become http;
    // `--config` reads flags from a file, which is `--disable`'s whole point
    // arriving by another road; `--proxy` and `--noproxy` move the endpoint;
    // `--unix-socket` and `--interface` move it further.
    const FORBIDDEN: &[&str] = &[
        "--insecure",
        "-k",
        "--proto-default",
        "--config",
        "-K",
        "--proxy",
        "-x",
        "--noproxy",
        "--unix-socket",
        "--interface",
        "--cacert",
        "--capath",
        "--ssl-allow-beast",
        "--doh-insecure",
        "--proxy-insecure",
        "--upload-file",
        "-T",
        "--data",
        "-d",
        "--remote-name",
        "-O",
        "--remote-header-name",
        "-J",
        "--write-out",
        "-w",
    ];
    for a in &as_str {
        if FORBIDDEN.contains(&a.as_str()) {
            return Err(format!("{a} must never appear in a Polylinker curl call"));
        }
    }

    if as_str.first().map(String::as_str) != Some("--disable") {
        return Err("--disable must be the first argument or curl still reads curlrc".into());
    }

    for required in [
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--tlsv1.2",
        "--globoff",
        "--connect-timeout",
        "--max-time",
        "--max-filesize",
        "--url",
    ] {
        if !as_str.iter().any(|a| a == required) {
            return Err(format!("{required} is missing"));
        }
    }

    // `--proto` and `--proto-redir` must both be present AND both be the
    // absolute form `=https`. `--proto https` (no `=`) is an *addition* to the
    // default set and permits everything else too, which is the subtle way to
    // get this wrong.
    for (flag, value) in [("--proto", "=https"), ("--proto-redir", "=https")] {
        match as_str.iter().position(|a| a == flag) {
            None => return Err(format!("{flag} is missing")),
            Some(i) => {
                if as_str.get(i + 1).map(String::as_str) != Some(value) {
                    return Err(format!("{flag} must be followed by {value}"));
                }
            }
        }
    }

    // The URL is the value of `--url`, is the last element, and is https.
    match as_str.iter().position(|a| a == "--url") {
        None => return Err("--url is missing".into()),
        Some(i) => {
            if i + 2 != as_str.len() {
                return Err("--url must be the last option and take exactly one value".into());
            }
            let url = &as_str[i + 1];
            if !url.starts_with("https://") {
                return Err(format!("{url} is not https"));
            }
            // A URL that is one argv element cannot smuggle a second one, but
            // whitespace or a control character in it means the builder let
            // something through that a `Version` could never contain.
            if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(format!("{url} contains whitespace or a control character"));
            }
            if url.contains("..") {
                return Err(format!("{url} contains a parent-directory hop"));
            }
        }
    }
    Ok(())
}

/// Is `program` the one this crate intends to run? Used only by the tests, and
/// stated here so the name appears once.
pub(crate) const PROGRAM: &str = "curl";

/// True if `curl` can be resolved and answers `--version`.
///
/// For a caller that wants to grey out a menu item rather than offer an update
/// and then explain that it cannot happen. Not used by [`Curl::get`], which
/// finds out by trying — a probe answers a question about a moment that has
/// already passed by the time the real call is made.
pub fn curl_available() -> bool {
    let mut cmd = Command::new(PROGRAM);
    cmd.arg("--version");
    no_console(&mut cmd);
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Keep Windows from opening a console window for `curl`.
///
/// `polylinker.exe` is built for the Windows GUI subsystem, so it has no
/// console of its own. Spawning a console program from it makes Windows create
/// one, and because `curl` finishes in well under a second what the user sees
/// is a black window that appears and vanishes. On a tool whose whole claim is
/// that it does not talk to the network unless asked, an unexplained terminal
/// flashing at launch is precisely the wrong thing to show — it looks like
/// exactly what a user would be right to be suspicious of.
///
/// This is not a new discovery in this repository. `bins/pl-gui/src/recover.rs`
/// already carries the same flag with the same reasoning for its `assoc` call,
/// and the note there says plainly that without it "a windowed build pops a
/// black cmd.exe window". The updater was written months later and did not
/// inherit it. Nobody had seen the flash because the update check is off by
/// default, so the defect was real and latent: it would have appeared for the
/// first user who ever switched the setting on, and it would have appeared at
/// launch, which is the least reassuring possible moment.
///
/// 0x0800_0000 is CREATE_NO_WINDOW. It is written out rather than pulled from a
/// crate because `crates/` take no dependencies.
#[cfg(windows)]
fn no_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Nothing to do: no other platform invents a terminal for a child process.
#[cfg(not(windows))]
fn no_console(_cmd: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(argv: &[OsString]) -> Vec<String> {
        argv.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    const URL: &str = "https://github.com/liorlobel/polylinker/releases/download/v1.2.3/x.txt";

    #[test]
    fn the_argument_vector_this_crate_actually_builds_passes_its_own_audit() {
        let curl = Curl::default();
        let memory = curl.argv(URL, Sink::Memory { max_bytes: 65536 }, 30);
        audit(&memory).expect("the in-memory fetch argv");

        let path = Path::new("C:\\Users\\somebody\\Downloads\\polylinker.msi.part");
        let file = curl.argv(
            URL,
            Sink::File {
                path,
                max_bytes: ARTIFACT_MAX_BYTES,
            },
            900,
        );
        audit(&file).expect("the download argv");

        // The output path is our own, is a single argv element, and is the
        // value of `--output`.
        let f = strs(&file);
        let i = f.iter().position(|a| a == "--output").expect("--output");
        assert_eq!(f[i + 1], path.to_string_lossy());

        // An in-memory fetch writes no file at all.
        assert!(!strs(&memory).iter().any(|a| a == "--output"));

        // The URL is one element and is not glued to anything.
        assert_eq!(f.last().unwrap(), URL);
        assert_eq!(strs(&memory).last().unwrap(), URL);
    }

    /// The audit is the test, so the audit is what has to be shown to fail.
    ///
    /// Every entry below is a plausible edit — a flag deleted during a
    /// refactor, a `--proto` written without its `=`, a `--insecure` added by
    /// somebody debugging a corporate TLS-inspecting proxy and left in, a URL
    /// concatenated onto a flag. If [`audit`] passed any of them, its use above
    /// would prove nothing.
    #[test]
    fn the_audit_rejects_the_argument_vectors_that_would_matter() {
        let good = Curl::default().argv(URL, Sink::Memory { max_bytes: 4096 }, 30);
        assert!(audit(&good).is_ok());

        let broken: Vec<(&str, Vec<OsString>)> = vec![
            ("--insecure added", {
                let mut v = good.clone();
                v.insert(1, OsString::from("--insecure"));
                v
            }),
            ("-k added", {
                let mut v = good.clone();
                v.insert(1, OsString::from("-k"));
                v
            }),
            ("a proxy added", {
                let mut v = good.clone();
                v.insert(1, OsString::from("--proxy"));
                v.insert(2, OsString::from("http://10.0.0.1:8080"));
                v
            }),
            ("a config file added", {
                let mut v = good.clone();
                v.insert(1, OsString::from("--config"));
                v.insert(2, OsString::from("/tmp/flags"));
                v
            }),
            ("--disable removed", good[1..].to_vec()),
            ("--disable no longer first", {
                let mut v = good.clone();
                let d = v.remove(0);
                v.insert(2, d);
                v
            }),
            ("--fail removed", {
                let mut v = good.clone();
                v.retain(|a| a != "--fail");
                v
            }),
            ("--location removed", {
                let mut v = good.clone();
                v.retain(|a| a != "--location");
                v
            }),
            ("--tlsv1.2 removed", {
                let mut v = good.clone();
                v.retain(|a| a != "--tlsv1.2");
                v
            }),
            ("--max-time removed", {
                let mut v = good.clone();
                let i = v.iter().position(|a| a == "--max-time").unwrap();
                v.drain(i..i + 2);
                v
            }),
            ("--proto without the =", {
                let mut v = good.clone();
                let i = v.iter().position(|a| a == "--proto").unwrap();
                v[i + 1] = OsString::from("https");
                v
            }),
            ("--proto-redir without the =", {
                let mut v = good.clone();
                let i = v.iter().position(|a| a == "--proto-redir").unwrap();
                v[i + 1] = OsString::from("https");
                v
            }),
            ("--proto-redir removed, so a redirect may leave https", {
                let mut v = good.clone();
                let i = v.iter().position(|a| a == "--proto-redir").unwrap();
                v.drain(i..i + 2);
                v
            }),
            ("an http URL", {
                let mut v = good.clone();
                *v.last_mut().unwrap() = OsString::from("http://github.com/x");
                v
            }),
            ("a file URL", {
                let mut v = good.clone();
                *v.last_mut().unwrap() = OsString::from("file:///etc/passwd");
                v
            }),
            ("a URL with a newline in it", {
                let mut v = good.clone();
                *v.last_mut().unwrap() = OsString::from("https://example.invalid/a\nb");
                v
            }),
            ("a URL with a parent-directory hop", {
                let mut v = good.clone();
                *v.last_mut().unwrap() = OsString::from("https://example.invalid/a/../../b");
                v
            }),
            ("the URL glued to the flag instead of passed as a value", {
                let mut v = good.clone();
                v.truncate(v.len() - 2);
                v.push(OsString::from(format!("--url={URL}")));
                v
            }),
            ("something appended after the URL", {
                let mut v = good.clone();
                v.push(OsString::from("--insecure"));
                v
            }),
            ("--max-filesize removed", {
                let mut v = good.clone();
                let i = v.iter().position(|a| a == "--max-filesize").unwrap();
                v.drain(i..i + 2);
                v
            }),
        ];

        for (what, argv) in broken {
            assert!(
                audit(&argv).is_err(),
                "the audit must reject an argv with {what}: {:?}",
                strs(&argv)
            );
        }
    }

    /// The program is `curl` and nothing is handed to a shell.
    ///
    /// The name is a constant so that this reads the same string the call does.
    /// A `Command::new("cmd")` or `Command::new("sh")` with a `-c` would be the
    /// injection this crate's URL handling is otherwise careful to make
    /// impossible, so it is asserted rather than left to review;
    /// `tests/handoff.rs` scans the sources for the same thing from the other
    /// side.
    #[test]
    fn the_program_is_curl() {
        assert_eq!(PROGRAM, "curl");
        assert!(!PROGRAM.contains("sh"));
        assert!(!PROGRAM.contains("cmd"));
    }

    /// The audit is wired into the call, not merely into the tests.
    ///
    /// [`Curl::run`] checks the argument vector before spawning anything, so an
    /// argv that would violate a rule never becomes a process. Asserted by
    /// handing `run` a vector that fails the audit and requiring
    /// [`UpdateError::UnsafeRequest`] back: any other answer — a transport
    /// error, a missing curl — would mean the process had been reached.
    ///
    /// The vector below has no `--url` in it, so nothing could be requested
    /// even if the guard were absent. That is deliberate: a test of a guard
    /// against network access must not itself depend on there being none.
    #[test]
    fn a_bad_argument_vector_never_reaches_the_process() {
        let curl = Curl::default();
        let bad = vec![OsString::from("--insecure")];
        let got = curl.run(&bad, "https://example.invalid/never-requested");
        match got {
            Err(UpdateError::UnsafeRequest { why }) => {
                assert!(why.contains("--insecure"), "{why}")
            }
            other => {
                panic!("a violating argv must be refused before curl is spawned, got {other:?}")
            }
        }
    }

    /// The timeouts are set, are finite, and the artifact allowance is the
    /// larger of the two.
    ///
    /// A zero `--max-time` means *no limit* to curl, which is precisely the
    /// hang these constants exist to prevent, and a `Default` written with a
    /// zero would look like a value rather than an absence.
    #[test]
    fn the_timeouts_are_finite_and_nonzero() {
        let c = Curl::default();
        assert!(c.connect_timeout_secs > 0);
        assert!(c.text_max_time_secs >= c.connect_timeout_secs);
        assert!(c.artifact_max_time_secs > c.text_max_time_secs);
        // Big enough for a real archive and small enough to be a limit. Both
        // ends, because "is it positive" would be satisfied by one byte. In a
        // `const` block because the value is a constant and clippy is right
        // that a runtime assertion on one is theatre — this way a bad edit
        // fails to compile rather than failing to run.
        const {
            assert!(ARTIFACT_MAX_BYTES > 64 * 1024 * 1024);
            assert!(ARTIFACT_MAX_BYTES < 4 * 1024 * 1024 * 1024);
        }
    }
}

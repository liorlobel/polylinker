//! A release version: three numbers, compared as numbers, and the only thing
//! the network is ever allowed to put into a URL.
//!
//! # Why this type exists rather than a `&str`
//!
//! `check` learns which version the server claims is current, and
//! `fetch_and_verify` builds `.../download/v{version}/SHA256SUMS.txt` out of
//! it. That is a string from the network reaching a command line, which is the
//! shape of every argument-injection bug there has ever been. The mitigation is
//! not to escape the string carefully at the point of use — that is a rule
//! somebody has to remember at every future call site — but to make the unsafe
//! form unrepresentable: **no function in this crate builds a URL from a
//! `&str`**, only from a [`Version`], and the only way to obtain a [`Version`]
//! is [`Version::parse`], which accepts nothing but three runs of ASCII digits
//! separated by dots.
//!
//! So `../../etc/passwd`, `-o/tmp/x`, `https://evil.example/`, `0.1.0 --insecure`
//! and `0.1.0\n` are not "escaped": they never become a `Version` at all, and
//! there is no other input to the URL builders.
//! `hostile_version_strings_are_refused` in `flow.rs` feeds those exact values
//! in and requires a refusal, and `a_version_renders_as_nothing_but_digits_and_dots`
//! closes the other end — that no *accepted* value can render anything a URL
//! would read as a path segment or a flag.
//!
//! # Why the comparison is numeric
//!
//! Lexically, `0.1.10` sorts before `0.1.2`, so a string comparison would tell
//! a user on 0.1.10 that 0.1.2 is an upgrade and offer to install it. That is
//! not merely a cosmetic bug: an attacker who can serve the release page could
//! use it to walk a user *backwards* onto an older release whose vulnerabilities
//! are public — and every byte of it would verify, because it is a genuine
//! release genuinely signed by the release key. A signature says who made the
//! bytes, never that they are the newest bytes, so the numeric comparison here
//! is the only thing standing between a signed archive and a rollback.
//! `numeric_ordering_is_not_lexical` pins the 0.1.2/0.1.10 case specifically.

use core::fmt;

/// The version compiled into this binary, from Cargo.
///
/// The workspace has exactly one version and every crate inherits it, so this
/// is the version of the whole application, not of this crate alone. That is
/// what makes it the right thing to compare a release against.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A three-part release version, ordered by number.
///
/// `PartialOrd`/`Ord` are derived, and that is correct rather than lazy: a
/// derived comparison on a struct compares fields in declaration order, and
/// major-then-minor-then-patch *is* the precedence order. The numbers are
/// integers, so `10 > 2` — the whole point. Nothing here is version-range
/// arithmetic; pre-release and build metadata are refused by the parser rather
/// than ordered, because this project has never published such a tag and
/// guessing at an ordering nobody has needed is how a rollback slips through.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// The longest string [`parse`](Version::parse) will look at.
    ///
    /// Three `u32`s cannot need more than 32 characters, and a bound stops a
    /// megabyte of digits served by a hostile endpoint from being walked at
    /// all. It is checked before anything else.
    const MAX_LEN: usize = 32;

    /// Build one from its parts, for tests and for callers that already have
    /// numbers rather than text.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// Parse `major.minor.patch`, and refuse everything else.
    ///
    /// Strict on purpose, in ways worth naming because each is a refusal
    /// somebody will eventually want to relax:
    ///
    /// * **No `v` prefix.** Tags are `v0.2.0` and Cargo versions are `0.2.0`;
    ///   this type is the Cargo form, and the `v` is added by the one function
    ///   that builds a tag URL. Accepting both would mean two spellings of one
    ///   version and a comparison that could disagree with itself.
    /// * **No leading zeros.** `01.2.3` and `1.2.3` would otherwise be two
    ///   strings for one version, and the one that round-trips is not the one
    ///   that arrived.
    /// * **No whitespace anywhere, not even trailing.** A trailing newline is
    ///   what a text file served over HTTP will have, so this looks like the
    ///   wrong choice — it is deliberate. Trimming is the caller's job and it
    ///   happens once, in `flow.rs`, where the surrounding bytes are visible;
    ///   a parser that quietly trimmed would also accept `0.1.0\n --insecure`
    ///   under some future `trim` that only removed the newline.
    /// * **ASCII digits only.** `is_ascii_digit`, not `char::is_numeric`, which
    ///   is true for Arabic-Indic digits, for `²`, and for a dozen other things
    ///   `u32::from_str` would then reject anyway — but only after they had
    ///   been through a length check written in bytes and a slice written in
    ///   chars.
    /// * **Exactly three parts.** `1.2` and `1.2.3.4` are refused rather than
    ///   padded or truncated.
    pub fn parse(text: &str) -> Option<Version> {
        if text.len() > Self::MAX_LEN {
            return None;
        }
        let mut parts = text.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = number(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }

    /// The version this binary was built as.
    ///
    /// `None` would mean `CARGO_PKG_VERSION` is not three numbers — a
    /// pre-release suffix in the workspace manifest, say. That is a build-time
    /// mistake rather than a runtime condition, so it is reported as an error
    /// and not a panic, and `the_compiled_in_version_parses` fails the build
    /// long before any user could meet it.
    pub fn current() -> Option<Version> {
        Version::parse(CURRENT_VERSION)
    }

    pub fn major(&self) -> u32 {
        self.major
    }
    pub fn minor(&self) -> u32 {
        self.minor
    }
    pub fn patch(&self) -> u32 {
        self.patch
    }
}

/// One dot-separated component: at least one ASCII digit, no leading zero
/// unless the whole component is `0`, and inside `u32`.
fn number(part: &str) -> Option<u32> {
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if part.len() > 1 && part.starts_with('0') {
        return None;
    }
    part.parse::<u32>().ok()
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0.1.2 > 0.1.10` is **false**, and a string comparison says otherwise.
    ///
    /// The one case in this file that is not obvious, and the one a rollback
    /// would ride in on. Both the type's ordering and the reason it is needed
    /// are asserted here: the second assertion shows the lexical comparison
    /// really does disagree, so this test is not merely restating what `Ord`
    /// does but pinning the difference between the two.
    ///
    /// `!(a > b)` is what the two negated assertions below mean, and clippy
    /// 1.94's `nonminimal_bool` wants `a <= b` instead. The two are equivalent
    /// for a total order and are not equivalent as statements of intent: what is
    /// under test is that `>` does NOT hold — the operator `fetch_and_verify`
    /// calls, and the one a lexical comparison gets wrong — rather than that
    /// some other operator does. Rewriting them would leave `>` unasserted at
    /// exactly the boundary each message names, so the lint is allowed instead.
    /// (Unrelated to the change this arrived in; it began firing when stable
    /// moved to 1.94.)
    #[allow(clippy::nonminimal_bool)]
    #[test]
    fn numeric_ordering_is_not_lexical() {
        let two = Version::parse("0.1.2").unwrap();
        let ten = Version::parse("0.1.10").unwrap();
        assert!(ten > two, "0.1.10 must be newer than 0.1.2");
        assert!(!(two > ten), "0.1.2 must NOT be newer than 0.1.10");
        assert!("0.1.2" > "0.1.10", "the trap this type exists to avoid");

        // The same shape one component to the left, and at the major.
        assert!(Version::parse("0.10.0").unwrap() > Version::parse("0.9.9").unwrap());
        assert!(Version::parse("10.0.0").unwrap() > Version::parse("9.99.99").unwrap());
        // Precedence: a larger patch never outranks a smaller minor.
        assert!(Version::parse("1.2.0").unwrap() > Version::parse("1.1.99").unwrap());
        // Equality is not newness. `fetch_and_verify` refuses an equal version,
        // so this is the boundary that decides whether a re-download happens.
        let same = Version::parse("1.2.3").unwrap();
        assert!(!(same > Version::parse("1.2.3").unwrap()));
    }

    #[test]
    fn a_well_formed_version_round_trips() {
        for text in ["0.0.0", "0.1.1", "1.2.3", "0.1.10", "4294967295.0.7"] {
            let v = Version::parse(text).expect(text);
            assert_eq!(v.to_string(), text);
        }
        assert_eq!(Version::new(1, 2, 3), Version::parse("1.2.3").unwrap());
    }

    /// Everything that is not three numbers is refused.
    ///
    /// The hostile half of this list is repeated in `flow.rs` against the URL
    /// builders, which is where it means something; here it is the parser's own
    /// contract. Both halves matter — the malformed-but-harmless cases (`1.2`,
    /// `1.2.3.4`) are what stop a partial parse silently inventing a version,
    /// and the hostile ones are what stop a URL being built out of a flag.
    #[test]
    fn anything_that_is_not_three_numbers_is_refused() {
        for bad in [
            "",
            ".",
            "..",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.",
            ".2.3",
            "1..3",
            "v1.2.3",
            "V1.2.3",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "+1.2.3",
            "-1.2.3",
            "1.-2.3",
            "1.2.3-beta",
            "1.2.3+build",
            "1.2.3 ",
            " 1.2.3",
            "1.2.3\n",
            "1.2.3\r\n",
            "1.2.3\t",
            "1.2.3\0",
            "1 .2.3",
            "1.2.3 --insecure",
            "1.2.3;rm -rf /",
            "../../x",
            "..",
            "-o/tmp/x",
            "https://evil.example/",
            "0.1.0/../../..",
            "0x1.2.3",
            "1e3.2.3",
            "١.٢.٣",
            "١23.2.3",
            "4294967296.0.0",
            "99999999999999999999.0.0",
            "1.2.3.",
            "1,2,3",
            "1-2-3",
        ] {
            assert!(
                Version::parse(bad).is_none(),
                "{bad:?} must not parse as a version"
            );
        }

        // Longer than MAX_LEN, refused before the split. Three components that
        // are each individually plausible, so only the length can reject it.
        let long = format!("{}.0.0", "1".repeat(64));
        assert!(Version::parse(&long).is_none());
    }

    /// No accepted version can render anything a URL or a command line would
    /// read as structure.
    ///
    /// The parser's job is to reject; this is the other half of the same claim,
    /// and it is the one a future edit is likely to break — relaxing `parse` to
    /// accept `1.2.3-rc1` would leave every test above passing and put a `-` in
    /// a string that is about to be concatenated into an argv element.
    #[test]
    fn a_version_renders_as_nothing_but_digits_and_dots() {
        for v in [
            Version::new(0, 0, 0),
            Version::new(1, 2, 3),
            Version::new(u32::MAX, u32::MAX, u32::MAX),
        ] {
            let s = v.to_string();
            assert!(
                s.bytes().all(|b| b.is_ascii_digit() || b == b'.'),
                "{s:?} contains something other than digits and dots"
            );
            assert!(!s.contains(".."), "{s:?} contains a parent-directory hop");
        }
    }

    /// The version this binary carries is one this crate can read.
    ///
    /// A workspace version of `0.2.0-rc1` would compile, ship, and make every
    /// update check fail at run time with a parse error. This turns that into a
    /// red test on the commit that introduces it.
    #[test]
    fn the_compiled_in_version_parses() {
        let v = Version::current().unwrap_or_else(|| {
            panic!("CARGO_PKG_VERSION is {CURRENT_VERSION:?}, which is not major.minor.patch")
        });
        assert_eq!(v.to_string(), CURRENT_VERSION);
    }
}

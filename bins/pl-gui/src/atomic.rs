//! One writer for every file the user asked this program to produce.
//!
//! # Why not `fs::write`
//!
//! `std::fs::write` opens the destination with `CREATE | WRITE | TRUNCATE`, so
//! the user's existing file is gone **before** the first byte of the new one is
//! offered to the disk. Every failure after that point — a full volume, a
//! disconnected share, a quota, a device error — leaves a truncated or partial
//! file where a complete one used to be, and the program has nothing left to
//! put back. Seven save and export paths were written that way — six calling
//! `fs::write` in `main.rs` (2952, 3107, 3288, 3383, 3500, 3526 at 713bd3b) and
//! `save_project` reaching the same call through `session::write`. On `Err` the
//! document stays dirty and in memory, so a retry survives; the file that was
//! being overwritten does not.
//!
//! Nine picker-driven save sites exist today — `grep -c '\.save_file()'` over
//! `main.rs` — and all nine come here. Seven of them predate this module; the
//! eighth, `export_map_png`, and the ninth, `export_protein`, were written
//! against it. The number is not maintained by hand:
//! `every_picker_driven_save_in_this_file_goes_through_atomic_write` recounts
//! it and goes red when this paragraph goes stale.
//!
//! # What this does instead
//!
//! Temp file beside the destination → `write_all` → `flush` → `sync_all` →
//! `rename` over the destination. The destination is never opened for writing,
//! so no window exists in which it is short. This is the same shape as
//! [`crate::recover::write`] and `pl_scan::store::save`, which is where the
//! wording of the durability rule below comes from — the user's own document
//! was the one file in this program not written that way.
//!
//! **Durability before visibility.** `sync_all` precedes the rename because a
//! rename that lands before the data does leaves a file that exists and is
//! empty, which is worse than a failed save: it looks like a saved document.
//!
//! # Two properties, and only one of them is claimed
//!
//! `session.rs`'s module doc records a measurement from this repo: on Windows,
//! eight threads renaming onto one destination all got `Ok(())`. A rename is
//! **not** a mutual exclusion, and nothing here treats it as one — there is no
//! claim, no winner and no marker. What is used is the other property: the
//! destination goes from its old contents to its new ones in one step and is
//! never observable as neither. That one holds.
//!
//! # What this costs, stated rather than discovered
//!
//! - A rename can fail where an in-place write would have succeeded. A process
//!   holding the destination open with write sharing but not delete sharing —
//!   the antivirus and sync clients `pl_scan::store::save` names — blocks the
//!   replace but not a truncating write. That failure leaves the destination
//!   intact, which is the property being bought, and the message says so.
//! - The file that ends up at the destination is a **new** file, so it carries
//!   default permissions rather than the ones the file it replaced had — the
//!   process umask on Unix, the directory's inherited ACL on Windows.
//!   `fs::write` kept both, because it never replaced anything.
//! - A read-only destination is refused **because [`write`] refuses it**, not
//!   because `rename` does. That is the correction to what this paragraph used
//!   to claim. The attribute governs the file on both platforms, but `rename`
//!   is an operation on the destination's *directory*, and POSIX `rename(2)`
//!   never consults the destination's mode. Measured on WSL2 against this
//!   module's own code, same file at 0444, before the guard existed:
//!
//!   ```text
//!   atomic::write -> Ok(())                              content "new", mode 0644
//!   fs::write     -> Err("Permission denied (os error 13)")  content intact
//!   ```
//!
//!   A reference plasmid a lab marked read-only was therefore safe from Save As
//!   on Windows and silently replaced on Linux and macOS. [`write`] now stats
//!   the destination and refuses before it stages anything, so the property is
//!   the same on all three. Two asymmetries survive, both narrower than the one
//!   they replace: a process with `CAP_DAC_OVERRIDE` (root) could have written
//!   through the mode bits and is now refused, and a file we cannot write for a
//!   reason the mode bits do not express — another user's 0644, a denying ACL —
//!   is not caught here, because `Permissions::readonly` is an attribute read
//!   and not an access check.
//! - A **symlinked** destination is followed, and that too is deliberate rather
//!   than inherited. `rename` acts on the destination's own directory entry, so
//!   before the resolution below, saving to `~/plasmids/pKoV.gb ->
//!   /mnt/lab-share/plasmids/pKoV.gb` destroyed the link and left the share
//!   holding the pre-edit sequence — measured on WSL2, `is_symlink` false
//!   afterwards, the new bytes at the link name, the old ones at the target.
//!   `fs::write` opened through the link, so this would have been a regression
//!   in exactly the case that hurts: a save that reports the path it was asked
//!   for while the shared copy never changes. [`write`] canonicalises a
//!   destination that is a link, which also puts the staging file on the
//!   target's volume where it belongs. **Hard links are not covered and cannot
//!   be** — a hard link has no target to resolve to, so the replace breaks it
//!   and the other name keeps the old contents. There is no rename-based fix.
//! - **What `sync_all` does and does not buy.** It is the file's data and
//!   metadata, not the parent directory's. On ext4, XFS and APFS that leaves
//!   the rename itself unsynced: after a power loss seconds after a save this
//!   program reported as written, the destination can hold its pre-save
//!   contents with the tab marked clean. This is strictly more durable than the
//!   `fs::write` it replaced and identical to [`crate::recover::write`] and
//!   `pl_scan::store::save`, so nothing regressed — but "atomic" here means
//!   *never observable as neither*, which is a visibility property, and it does
//!   not mean *survives the power going out*. The missing step is an fsync of
//!   the parent, and it is left out rather than forgotten: `File::open(parent)`
//!   then `sync_all` is the Unix spelling and Windows has no portable
//!   equivalent — a directory handle there needs `CreateFile` with
//!   `FILE_FLAG_BACKUP_SEMANTICS`, which is raw platform code this crate does
//!   not otherwise carry. If it is ever added it must be added to all three
//!   call sites, or the guarantee differs by which file you saved.
//! - A crash between `File::create` and `rename` leaves one `*.pl-tmp` beside
//!   the user's file. Both error paths remove it; a killed process cannot.
//!   Nothing sweeps these, deliberately: the directory is one the user chose,
//!   not one this program owns, and a save-time sweep of somebody's Documents
//!   folder is a worse thing to be wrong about than a stray temp.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The temporary a save to `path` is staged through.
///
/// **Beside the destination, never in the temp directory.** `fs::rename` across
/// volumes is not a rename; staging `D:\work\p.dna` under `C:\Users\…\Temp`
/// would fail on every machine whose data does not live on the boot disk, which
/// for this program's users is most of them.
///
/// The process id is in the name because `File::create` on Windows opens with
/// `FILE_SHARE_WRITE`: two Polylinker windows saving the same path through one
/// shared temp name would interleave into a single file and both report
/// success. That reasoning is `pl_scan::store::save`'s, and it is why this is
/// not simply `path.with_extension("pl-tmp")`.
///
/// No per-save counter, unlike `store::save`, and the difference is deliberate:
/// `store` is written from a scan's worker threads, while every caller here
/// runs on the egui update thread, one save at a time. Leaving the counter out
/// makes the name a pure function of the destination and this process, which is
/// what lets a test put something in its way.
pub fn temp_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "polylinker".into());
    path.with_file_name(format!("{name}.{}.pl-tmp", std::process::id()))
}

/// The destination a save actually lands on: `path`, or what it points at.
///
/// A rename replaces a directory entry, so an unresolved symlink destination
/// would be destroyed rather than written through — see the costs list. Only a
/// destination that *is* a link is canonicalised, for two reasons: a save to a
/// path that does not exist yet must not become an error (that is every Save
/// As), and `canonicalize` on Windows returns a `\\?\` verbatim path, which
/// would otherwise turn up in ordinary error messages for no benefit.
///
/// A link whose target cannot be resolved — a dangling link, a share that is
/// not mounted — falls back to the path as given, so the save is refused by the
/// same code that refuses any other unwritable destination rather than by a
/// separate one here.
fn through_links(path: &Path) -> PathBuf {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

/// Refuse a read-only destination, which `rename` on its own would replace.
///
/// The whole justification is in the costs list; what is here is the shape.
/// A destination that does not exist is the normal case and must not be an
/// error, so anything other than `Ok(metadata)` proceeds — a missing file, an
/// unreadable parent, a dangling link all fail later, at `File::create` or at
/// `rename`, with the OS's own message rather than a guess from here.
///
/// `is_file` is part of the test rather than an accident. On Windows
/// `FILE_ATTRIBUTE_READONLY` on a *directory* does not mean read-only at all —
/// it marks a customised folder — so without it a directory destination would
/// be refused with a sentence about read-only files instead of the OS's
/// accurate one.
fn refuse_read_only(path: &Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.permissions().readonly() => Err(format!(
            "{}: the file is read-only — it is unchanged",
            path.display()
        )),
        _ => Ok(()),
    }
}

/// Write `bytes` to `path`, or leave `path` exactly as it was.
///
/// The `Err` string is ready to show: it names the file the failure was about,
/// quotes the OS, and — because the whole point of this module is that the user
/// has not lost anything — says that the destination is unchanged. A save that
/// reports failure while having destroyed the file it was overwriting is the
/// defect this replaces, and a message that does not mention the destination is
/// how that defect stayed invisible.
///
/// The read-only check comes **before** the staging file, not before the
/// rename. Refusing at the rename would be equally safe for the user's data but
/// would leave a `*.pl-tmp` beside a file the program just declined to touch,
/// and it would have written the whole document to disk to learn something one
/// `stat` answers.
pub fn write(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), String> {
    let bytes = bytes.as_ref();
    let path = &through_links(path);
    refuse_read_only(path)?;
    let tmp = temp_beside(path);

    let stage = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        // Durability before visibility: see the module doc.
        f.sync_all()?;
        Ok(())
    };
    if let Err(e) = stage() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "{}: {e} — {} is unchanged",
            tmp.display(),
            path.display()
        ));
    }

    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e} — the previous file is unchanged", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pl-gui-atomic-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a temp directory");
        d
    }

    /// PROVEN TO FAIL against `std::fs::write`, which is what all seven of the
    /// save paths this module replaced reached — **six** of them calling it in
    /// `main.rs` and `save_project` arriving through `session::write`, not
    /// seven in `main.rs`, which is what this used to say. The destination came
    /// back holding `b"new"` and the call returned `Ok(())`, having reported
    /// success for a write that never happened and destroyed the file it was
    /// overwriting on the way.
    ///
    /// The failure is injected where a real one lands — after the caller has
    /// committed to saving, at the point the bytes meet the disk — by putting a
    /// directory at the staging name so `File::create` cannot have it. A full
    /// volume, a quota or a dead network share arrive at the same place.
    #[test]
    fn a_save_that_fails_leaves_the_destination_byte_for_byte() {
        let d = dir("fail");
        let dest = d.join("plasmid.gb");
        std::fs::write(&dest, b"the user's only copy").unwrap();

        // In the way of the staging file, and nothing else.
        std::fs::create_dir_all(temp_beside(&dest)).unwrap();

        let e = write(&dest, b"new").expect_err("the write cannot have succeeded");
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            b"the user's only copy",
            "the destination was truncated by a save that failed"
        );
        assert!(
            e.contains("plasmid.gb") && e.contains("unchanged"),
            "the message must name the destination and say it survived: {e:?}"
        );
    }

    /// The ordinary path: the bytes land, and the staging file does not stay.
    ///
    /// This one cannot fail against `fs::write`, which got the ordinary path
    /// right — so it was PROVEN TO FAIL against the likeliest wrong version of
    /// the fix instead. Swapping the `rename` for `fs::copy` leaves the file
    /// correct and the temp in place, and the run reported exactly that:
    /// `a temporary was left beside the file: ["…\plasmid.gb.7048.pl-tmp"]`.
    /// Litter in the directory the user chose is the cost of staging there, and
    /// this is what holds the removal to it.
    #[test]
    fn a_save_that_succeeds_replaces_the_bytes_and_leaves_nothing_behind() {
        let d = dir("ok");
        let dest = d.join("plasmid.gb");
        std::fs::write(&dest, b"old").unwrap();

        write(&dest, "LOCUS       new").expect("the write");
        assert_eq!(std::fs::read(&dest).unwrap(), b"LOCUS       new");

        let left: Vec<PathBuf> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p != &dest)
            .collect();
        assert!(
            left.is_empty(),
            "a temporary was left beside the file: {left:?}"
        );
    }

    /// A read-only destination is still refused — on every platform, which it
    /// was not when this test was written.
    ///
    /// Replacing a write with a rename could plausibly have walked straight
    /// past the read-only attribute — the new file is the one being created,
    /// and nothing opens the old one. On Windows it does not, and that much was
    /// measured: on Windows 11, on the same file, both calls refuse.
    ///
    /// ```text
    /// fs::write  -> Err(Os { code: 5, kind: PermissionDenied, … })
    /// atomic     -> Err("…\locked.gb: Access is denied. (os error 5) — …")
    /// ```
    ///
    /// The mistake was generalising from that. `rename` is a directory
    /// operation and POSIX does not consult the destination's mode, so this
    /// test — carrying a bare `#[test]`, in a module `main.rs:13` includes
    /// unconditionally, run by `.github/workflows/ci.yml` on a three-OS matrix
    /// that had never executed — would have gone red on two runners of the
    /// first push. PROVEN TO FAIL, by running this exact file on Linux:
    ///
    /// ```text
    /// $ cargo test          # ~/pl-atomic-probe, #[path]-including atomic.rs
    /// test atomic::tests::a_read_only_destination_is_refused_exactly_as_before ... FAILED
    /// panicked at bins/pl-gui/src/atomic.rs:221:38:
    /// a read-only file must not be replaced: ()
    /// ```
    ///
    /// and the measurement behind the refusal, same file at 0444:
    ///
    /// ```text
    /// atomic::write -> Ok(())            content "new", mode 0444 -> 0644
    /// fs::write     -> Err(os error 13)  content "reference plasmid"
    /// ```
    ///
    /// So the choice was between a `#[cfg(windows)]` on this test and a guard
    /// in `write`, and it went to the guard: the thing the platform-conditional
    /// test would have documented is a reference plasmid a lab marked read-only
    /// being silently replaced, which is the loss this module exists to
    /// prevent. Making the test conditional would have recorded the data-loss
    /// path rather than closing it.
    ///
    /// Also PROVEN TO FAIL against the hardening that would break it on
    /// Windows — clearing the attribute before the rename, which is what
    /// someone chasing the "access denied" report will reach for first.
    #[test]
    // The clearing below is cleanup of a file this test made read-only two lines
    // earlier, in a per-process directory under `temp_dir` that the next run
    // deletes. Clippy's objection — that this is world-writable on Unix — is
    // about permanent files and there is no permanent file here.
    #[allow(clippy::permissions_set_readonly_false)]
    fn a_read_only_destination_is_refused_exactly_as_before() {
        let d = dir("ro");
        let dest = d.join("locked.gb");
        std::fs::write(&dest, b"reference plasmid").unwrap();
        let mut perm = std::fs::metadata(&dest).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&dest, perm).unwrap();

        let e = write(&dest, b"new").expect_err("a read-only file must not be replaced");
        assert!(e.contains("locked.gb"), "{e:?}");
        assert_eq!(std::fs::read(&dest).unwrap(), b"reference plasmid");
        // The refusal is before the staging file, not at the rename: a file
        // this program declined to touch must not gain a `.pl-tmp` sibling.
        assert!(
            !temp_beside(&dest).exists(),
            "the refusal staged the document anyway"
        );

        // Or the directory cannot be cleaned up on the next run.
        let mut perm = std::fs::metadata(&dest).unwrap().permissions();
        perm.set_readonly(false);
        std::fs::set_permissions(&dest, perm).unwrap();
    }

    /// A symlinked destination is written **through**, not replaced.
    ///
    /// `#[cfg(unix)]` for a reason that is not the one B4 was about. There the
    /// platforms genuinely behaved differently and the conditional would have
    /// documented a defect; here the behaviour is the same on Windows and the
    /// *harness* is not — `std::os::windows::fs::symlink_file` needs
    /// SeCreateSymbolicLinkPrivilege or Developer Mode, so a Windows version of
    /// this test would skip on most machines, and a test that cannot fail is
    /// worse than one that says which platform it covers. `through_links` is
    /// not conditional and does the same thing to a Windows file symlink; what
    /// is untested there is the OS, not the branch.
    ///
    /// PROVEN TO FAIL against the unresolved `rename` this replaces. Dropping
    /// `through_links` from [`write`] and re-running on Linux:
    ///
    /// ```text
    /// test atomic::tests::a_symlinked_destination_is_followed_rather_than_replaced ... FAILED
    /// the save replaced the link with a regular file
    /// ```
    ///
    /// and the same case measured in full, before the fix:
    ///
    /// ```text
    /// atomic::write(link) -> Ok(())
    ///   link is symlink : false          <- the link is gone
    ///   link content    : "edited"       <- a private regular file
    ///   target content  : "the shared sequence"   <- the share never changed
    /// ```
    ///
    /// against `fs::write`, which is what all seven converted sites did — the
    /// six that called it directly and `save_project` through `session::write`:
    ///
    /// ```text
    ///   link is symlink : true
    ///   target content  : "edited"
    /// ```
    ///
    /// The lab-share case in the costs list is this one: the status line names
    /// the path it was asked for, the file at that path holds the new bases,
    /// and the copy everyone else opens does not.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_is_followed_rather_than_replaced() {
        let d = dir("link");
        std::fs::create_dir_all(d.join("share")).unwrap();
        let target = d.join("share").join("pKoV.gb");
        let link = d.join("pKoV.gb");
        std::fs::write(&target, b"the shared sequence").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write(&link, b"edited").expect("the write");

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the save replaced the link with a regular file"
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"edited",
            "the bytes went to the link's own name instead of the share"
        );
        // And the staging file went to the target's directory, so the rename
        // never had to cross whatever the link points at.
        assert!(!temp_beside(&link).exists());
        assert!(!temp_beside(&target).exists());
    }

    /// A destination that does not exist yet is created, which is what every
    /// Save As and every first figure export does.
    ///
    /// Also unable to fail against `fs::write`, and PROVEN TO FAIL against the
    /// obvious next change to this module: reading the destination's metadata
    /// before staging, to carry its mode across. That is a sound instinct — a
    /// staged file gets fresh default permissions rather than the ones the file
    /// being replaced had — and implementing it with a `?` on the `metadata`
    /// call turns every save-to-a-new-name into `os error 2`. Whoever adds it
    /// must treat a missing destination as the normal case.
    #[test]
    fn a_save_to_a_new_name_creates_it() {
        let d = dir("new");
        let dest = d.join("figure.svg");
        write(&dest, "<svg/>").expect("the write");
        assert_eq!(std::fs::read(&dest).unwrap(), b"<svg/>");
    }
}

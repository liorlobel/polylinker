//! The Dock tile: whether the bundle this process runs from already gives it
//! an icon, and what AppKit is holding for it once eframe has had its say.
//!
//! This and `macmenu.rs` are the two files in this binary with an `objc2` in
//! them, and like that one this is measurement with no policy: the decision
//! it feeds — which icon `start` hands `ViewportBuilder::with_icon` — is
//! `startup_icon` in `main.rs`, a function of one `bool`, where the Linux and
//! Windows CI legs can test it.
//!
//! # The defect this closes (2026-09-06)
//!
//! Launched from `Polylinker.app`, the Dock drew
//! `Contents/Resources/Polylinker.icns` — eleven PNGs, up to 1024 px, from
//! `icon/build-icns.py` — and then, one frame later, something else. That
//! something was eframe: `AppTitleIconSetter::update` runs from `pre_update`
//! on every frame until it succeeds (eframe-0.35.0/src/native/
//! epi_integration.rs:260), and on macOS `set_title_and_icon_mac` builds one
//! `NSBitmapImageRep` from the pixels `with_icon` was given, wraps it in an
//! `NSImage` of exactly that size and calls `setApplicationIconImage:`
//! (app_icon.rs:247-274). The pixels were `main.rs`'s 64 px window icon, so
//! the Dock's 1024 px drawing became a 64 px bitmap scaled up, and the tile
//! visibly changed after launch. The same swap reaches the ⌘Tab switcher and
//! the About panel; Finder keeps reading the `.icns` and never showed it.
//!
//! eframe's off switch is `IconData::default()`, and it is documented as one
//! (eframe-0.35.0/src/epi.rs:295-296; egui-0.35.0/src/viewport.rs:433):
//! `AppTitleIconSetter::new` turns exactly that value into `None`
//! (app_icon.rs:17-21), `set_title_and_icon_mac` then leaves the icon alone
//! and still retitles the first menu — ours, already called "Polylinker" —
//! and egui-winit's `to_winit_icon` returns `None` for an empty icon with no
//! warning (egui-winit-0.35.0/src/lib.rs:2165). Leaving the icon out of
//! `NativeOptions` altogether is NOT the off switch: that is the egui logo
//! (epi_integration.rs:200-204), which is how this binary got its own icon in
//! the first place.
//!
//! So `startup_icon` hands eframe that value when, and only when,
//! [`bundle_declares_icon`] says the bundle names an icon. A bare
//! `polylinker` run from the tarball has no bundle and no icon, and the Dock
//! would show the generic executable tile; the 64 px bitmap is the better of
//! the two there, and it stays.
//!
//! # Measured, not reasoned about
//!
//! [`check_under_smoke`] is the measurement. `PL_GUI_SMOKE=1` opens a real
//! window, and on its first frame — after `pre_update`, so after eframe has
//! set the icon or not — this reads `NSApplication.applicationIconImage`
//! back and compares its size with the one thing eframe would have put
//! there: an `ICON_PX` × `ICON_PX` image (measured: eframe's reads back as
//! 64x64 pt, the `.icns` as 128x128). Both launches run on every push:
//! `.github/workflows/ci.yml`'s `gui-smoke` job runs the bare binary, where
//! the icon must be eframe's, and its macOS `test` leg runs
//! `tools/check-dmg.sh --launch`, which starts `Contents/MacOS/polylinker`
//! from the mounted image, where it must not be. A wrong answer exits 3,
//! which `check-dmg.sh` reports as "exited 3" and the smoke job as a failed
//! step.

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::{ns_string, NSBundle};

/// Does the bundle this process was launched from name an icon in its
/// `Info.plist`?
///
/// `NSBundle.mainBundle` is derived from the executable's path, so this is
/// the `.app` whether the program was double-clicked or its
/// `Contents/MacOS/polylinker` was run from a shell, as `check-dmg.sh
/// --launch` does. For an executable with no wrapper it is the executable's
/// directory, which has no `Info.plist`, and every key reads as absent.
pub fn bundle_declares_icon() -> bool {
    declares_icon(&NSBundle::mainBundle())
}

/// `CFBundleIconFile` is the key LaunchServices reads to draw the tile before
/// the process exists, and the one `tools/build-dmg.sh` writes;
/// `CFBundleIconName` is the asset-catalogue spelling, written beside it.
/// Either is a bundle with an icon of its own.
fn declares_icon(bundle: &NSBundle) -> bool {
    bundle
        .objectForInfoDictionaryKey(ns_string!("CFBundleIconFile"))
        .is_some()
        || bundle
            .objectForInfoDictionaryKey(ns_string!("CFBundleIconName"))
            .is_some()
}

/// The size, in points, of the image AppKit holds as this process's Dock
/// icon.
///
/// Before anyone sets one this is the bundle's icon — or, with no bundle, the
/// generic application tile — and after eframe's `setApplicationIconImage:`
/// it is the `NSImage` eframe built, whose size is exactly the pixel size of
/// the `IconData` it was given.
fn dock_icon_size(mtm: MainThreadMarker) -> Option<(f64, f64)> {
    let image = NSApplication::sharedApplication(mtm).applicationIconImage()?;
    let size = image.size();
    Some((size.width, size.height))
}

/// The smoke leg's macOS question: is the Dock holding the icon it should?
///
/// Called from `App::ui` on the smoke run's first frame and nowhere else.
/// `ours_px` is the edge of the icon `startup_icon` hands eframe on a bare
/// launch, `ICON_PX`; an icon of exactly that size is eframe's, because
/// nothing else puts a 64-point image there. Measured 2026-09-06, on this
/// Mac, from a scratch bundle and a bare debug binary: eframe's image reads
/// back as 64x64 pt; the bundle's `.icns` as 128x128; and so does the generic
/// tile a bare executable is left with when eframe is handed no icon (forced
/// by mutation — the PROVEN TO FAIL note on `main.rs`'s
/// `a_bundle_with_its_own_icon_hands_eframe_no_icon` has the other half).
/// So the size tells eframe's image from either of AppKit's and does not tell
/// those two apart, which this never needs: when eframe leaves a declared
/// icon alone, LaunchServices drew the `.icns`. Says what it measured either
/// way, and exits 3 when the bundle declares an icon and the Dock holds
/// eframe's, or declares none and the Dock does not.
pub fn check_under_smoke(bundle_declares_icon: bool, ours_px: u32) {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("Polylinker: smoke: not on the main thread, so the Dock icon was not read");
        std::process::exit(3);
    };
    let Some((w, h)) = dock_icon_size(mtm) else {
        eprintln!("Polylinker: smoke: NSApplication holds no icon image at all");
        std::process::exit(3);
    };
    let eframes = w == f64::from(ours_px) && h == f64::from(ours_px);
    eprintln!(
        "Polylinker: smoke: the Dock icon is {w}x{h} pt, {}; the bundle {}",
        if eframes {
            "eframe's bitmap from with_icon"
        } else {
            "not eframe's"
        },
        if bundle_declares_icon {
            "declares an icon"
        } else {
            "declares none"
        }
    );
    if bundle_declares_icon && eframes {
        eprintln!(
            "Polylinker: smoke: FAIL -- the bundle has its own icon and eframe replaced it \
             with the {ours_px} px window icon; startup_icon should have handed eframe \
             IconData::default()"
        );
        std::process::exit(3);
    }
    if !bundle_declares_icon && !eframes {
        eprintln!(
            "Polylinker: smoke: FAIL -- there is no bundle icon and eframe did not set the \
             {ours_px} px one, so the Dock is showing the generic tile"
        );
        std::process::exit(3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSString;

    /// The test binary lives in `target/.../deps` with no wrapper around it,
    /// so this is the bare-launch answer, from the real `mainBundle`.
    #[test]
    fn a_bare_executable_declares_no_icon() {
        assert!(
            !bundle_declares_icon(),
            "the test binary is not inside a .app, and yet its main bundle names an icon"
        );
    }

    /// The two keys LaunchServices reads are the two that count, and nothing
    /// else in a plist does.
    ///
    /// Three scratch bundles, each a directory ending in `.app` with a
    /// `Contents/Info.plist` and nothing else — `NSBundle` needs no
    /// executable to answer a plist question — read through `bundleWithPath`,
    /// which is the same lookup `mainBundle` does for the running process.
    ///
    /// PROVEN TO FAIL: with `declares_icon` reading `CFBundleIconFiles` (one
    /// letter more) the first assertion fails with "ByFile.app names
    /// CFBundleIconFile and was read as having no icon".
    #[test]
    fn the_keys_launchservices_reads_are_the_ones_that_count() {
        let root = std::env::temp_dir().join(format!("pl-macdock-{}", std::process::id()));
        let make = |name: &str, keys: &str| -> String {
            let app = root.join(name);
            let contents = app.join("Contents");
            std::fs::create_dir_all(&contents).unwrap();
            std::fs::write(
                contents.join("Info.plist"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                     \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                     <plist version=\"1.0\"><dict>\n\
                     <key>CFBundleExecutable</key><string>nothing</string>\n\
                     <key>CFBundlePackageType</key><string>APPL</string>\n\
                     {keys}\n\
                     </dict></plist>\n"
                ),
            )
            .unwrap();
            app.to_str().unwrap().to_owned()
        };
        let by_file = make(
            "ByFile.app",
            "<key>CFBundleIconFile</key><string>Polylinker</string>",
        );
        let by_name = make(
            "ByName.app",
            "<key>CFBundleIconName</key><string>Polylinker</string>",
        );
        let neither = make("Neither.app", "");
        let bundle = |path: &str| {
            NSBundle::bundleWithPath(&NSString::from_str(path))
                .expect("a directory that exists is a bundle, plist or not")
        };

        assert!(
            declares_icon(&bundle(&by_file)),
            "ByFile.app names CFBundleIconFile and was read as having no icon"
        );
        assert!(
            declares_icon(&bundle(&by_name)),
            "ByName.app names CFBundleIconName and was read as having no icon"
        );
        assert!(
            !declares_icon(&bundle(&neither)),
            "Neither.app names no icon and was read as having one"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}

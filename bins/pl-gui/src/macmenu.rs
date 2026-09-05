//! The native macOS menu bar: [`crate::menu::MENUS`] turned into `NSMenu`
//! objects, and nothing else.
//!
//! This is the only file in this binary with an `objc2` in it, and it is a
//! LOOP WITH NO POLICY: every label, chord, ordering decision and reason lives
//! in `menu.rs`, where the Linux and Windows CI legs can read it. What this
//! file decides is how a `menu::Entry` becomes an `NSMenuItem`, which selector
//! a `menu::Standard` maps to, and how a click gets back into the frame — and
//! each of those has one answer here.
//!
//! # Three measurements this file is built on (2026-09-05)
//!
//! **winit installs a menu bar of its own**, from `applicationDidFinishLaunching`
//! (winit-0.30.13, `platform_impl/macos/app_state.rs:142` → `menu.rs:86`,
//! `setMainMenu`), which runs after `main()` and BEFORE the eframe creator
//! closure. A five-menu bar installed ahead of `run_native` was, by the time
//! `App::new` ran, winit's one-menu default. So [`install`] runs from `App::new`
//! and nowhere earlier. Replacing that menu also removes its Hide item — ⌘H
//! went dead the moment a bar without one took over — which is why
//! `menu::MENUS` carries every standard item winit's did.
//!
//! **An `NSMenuItem` does not retain its target.** `target` is a weak
//! reference (objc2-app-kit `NSMenuItem.rs`, `setTarget`), so if the
//! `Retained<MenuTarget>` were dropped the menu would message freed memory on
//! the next click. It cannot live in a `static Mutex`: the class is
//! `MainThreadOnly`, so its `Retained` is `!Send` and that does not compile. A
//! main-thread thread-local is the honest home, and the `MainThreadMarker`
//! every function here takes is the proof we are on that thread.
//!
//! **A callback must not touch the frame.** Re-entering winit from inside an
//! action aborts the process (`event_handler.rs:135`, exit 134). So
//! [`MenuTarget::pl_command`] pushes into `menu::push` and returns; the frame
//! drains it. See `menu::QUEUE`.
//!
//! # What is not here
//!
//! No `undo:`, `cut:`, `copy:`, `paste:` or `selectAll:`. Those selectors go to
//! the responder chain, the first responder is winit's `NSView`, and it answers
//! none of them (measured: `respondsToSelector:` false for all five). Items
//! built on them are permanently grey — and a disabled item STILL takes its key
//! equivalent, so a conventional Edit menu killed ⌘C and ⌘A in every text box
//! in the window, measured A/B with and without it. Every item of ours is a
//! target/action item into Rust, and the text-editing chords stay with egui.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::menu::{self, Action, Chord, Command, Entry, Origin, Show, Standard};

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and this type has no
    // `Drop` impl and no ivars that need one. `MainThreadOnly` because every
    // caller is AppKit's menu machinery on the main thread, and it is what lets
    // `alloc` take a `MainThreadMarker` rather than trusting the call site.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct MenuTarget;

    impl MenuTarget {
        /// Every item of ours points here. Which command is decided by the
        /// item's tag, and the frame decides everything else.
        #[unsafe(method(plCommand:))]
        fn pl_command(&self, sender: Option<&NSMenuItem>) {
            let Some(item) = sender else { return };
            let Some(cmd) = Command::from_tag(item.tag()) else { return };
            menu::push(cmd, origin(self.mtm()));
        }

        /// AppKit's automatic enabling asks this for every item whose target
        /// we are, each time a menu is about to be shown. The answer is the
        /// mask `App::commands` published on the last frame — the same
        /// `Gate::allows` and `App::can` a click would be judged by — so an
        /// item is grey for exactly the reason its click would be refused.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: Option<&NSMenuItem>) -> bool {
            match item.and_then(|i| Command::from_tag(i.tag())) {
                Some(cmd) => menu::enabled(cmd),
                // Not ours — a separator, or a standard item that happens to
                // ask. Never grey what we did not install.
                None => true,
            }
        }
    }

    unsafe impl NSObjectProtocol for MenuTarget {}
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `init` on an `NSObject` subclass with no custom init.
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// The one target, kept alive for the life of the process. See the module
    /// header for why it is here and not in a `static`.
    static TARGET: RefCell<Option<Retained<MenuTarget>>> = const { RefCell::new(None) };
}

/// Was the action that is running right now started by a keystroke?
///
/// `NSApplication.currentEvent` is the event being dispatched. When a key
/// equivalent matched, that is the key-down AppKit intercepted; when a person
/// clicked, it is the mouse-up that closed the menu. The one imprecision is a
/// menu walked with the arrow keys and confirmed with Return, whose current
/// event is also a key-down: that reads as `KeyEquivalent`, and so a Return on
/// File ▸ Save while a text box has the caret is refused as a ⌘S there would
/// be. Rare, and the failure is a click that does nothing rather than a guard
/// that is bypassed, which is the right way round.
fn origin(mtm: MainThreadMarker) -> Origin {
    let app = NSApplication::sharedApplication(mtm);
    match app.currentEvent() {
        Some(ev) if ev.r#type() == NSEventType::KeyDown => Origin::KeyEquivalent,
        _ => Origin::Click,
    }
}

/// The AppKit selector a standard item sends, target nil, up the responder
/// chain. Each was measured enabled on a live winit window on 2026-09-05:
/// `hide:` and its two siblings answer on `NSApplication`,
/// `performMiniaturize:` and `performZoom:` on `NSWindow`.
fn standard_selector(s: Standard) -> Sel {
    match s {
        Standard::Hide => sel!(hide:),
        Standard::HideOthers => sel!(hideOtherApplications:),
        Standard::ShowAll => sel!(unhideAllApplications:),
        Standard::Minimize => sel!(performMiniaturize:),
        Standard::Zoom => sel!(performZoom:),
        Standard::BringAllToFront => sel!(arrangeInFront:),
        // The Services item carries a submenu AppKit fills and no action of
        // its own; `install` handles it before asking for a selector.
        Standard::Services => sel!(hide:),
    }
}

/// The modifier mask for a chord, set EXPLICITLY rather than left to
/// `NSMenuItem`'s default of Command. The default is wrong for F1 and F3, which
/// this application binds bare, and relying on an upper-case letter to imply
/// Shift would put the chord's modifiers inside its spelling.
fn modifier_mask(c: Chord) -> NSEventModifierFlags {
    let mut m = NSEventModifierFlags::empty();
    if c.cmd {
        m |= NSEventModifierFlags::Command;
    }
    if c.shift {
        m |= NSEventModifierFlags::Shift;
    }
    if c.alt {
        m |= NSEventModifierFlags::Option;
    }
    m
}

/// One `NSMenuItem` from one entry.
fn build_item(
    mtm: MainThreadMarker,
    label: &str,
    action: Action,
    binds: &[menu::Bind],
    target: &MenuTarget,
) -> Retained<NSMenuItem> {
    // At most one chord is printed — `an_item_prints_at_most_one_chord` — and
    // a withheld one is exactly that: not installed.
    let shown = binds
        .iter()
        .find(|b| b.show == Show::KeyEquivalent)
        .map(|b| b.chord);
    let key = shown.and_then(Chord::key_equivalent).unwrap_or("");
    let selector = match action {
        Action::App(_) => sel!(plCommand:),
        Action::System(s) => standard_selector(s),
    };
    // SAFETY: the title and key equivalent are valid `NSString`s built from
    // `&str`, and the selector names a method either this class or the
    // responder chain implements.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(label),
            Some(selector),
            &NSString::from_str(key),
        )
    };
    if let Some(chord) = shown {
        item.setKeyEquivalentModifierMask(modifier_mask(chord));
    }
    if let Action::App(cmd) = action {
        item.setTag(cmd.tag());
        // SAFETY: the target outlives the menu — it is parked in `TARGET` for
        // the life of the process, per the module header.
        unsafe { item.setTarget(Some(target as &MenuTarget as &AnyObject)) };
    }
    item
}

/// Build `menu::MENUS` and make it the application's main menu.
///
/// Returns whether it did. `false` means "not on the main thread", which
/// cannot happen from `App::new` and is reported rather than panicked on
/// because a menu bar is not worth crashing an editor over.
///
/// Called from `App::new`, and not before `run_native`: see the module header
/// for the measurement that decided where.
pub fn install() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let target = MenuTarget::new(mtm);
    let bar = NSMenu::new(mtm);

    for m in menu::MENUS {
        let top = NSMenuItem::new(mtm);
        let sub = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(m.title));
        for e in m.entries {
            match e {
                Entry::Separator => sub.addItem(&NSMenuItem::separatorItem(mtm)),
                Entry::Item {
                    label,
                    action: Action::System(Standard::Services),
                    ..
                } => {
                    // The Services submenu is AppKit's to fill; ours is the
                    // slot it fills. The item itself sends nothing.
                    let item = unsafe {
                        NSMenuItem::initWithTitle_action_keyEquivalent(
                            NSMenuItem::alloc(mtm),
                            &NSString::from_str(label),
                            None,
                            &NSString::from_str(""),
                        )
                    };
                    let services = NSMenu::new(mtm);
                    item.setSubmenu(Some(&services));
                    app.setServicesMenu(Some(&services));
                    sub.addItem(&item);
                }
                Entry::Item {
                    label,
                    action,
                    binds,
                } => sub.addItem(&build_item(mtm, label, *action, binds, &target)),
            }
        }
        top.setSubmenu(Some(&sub));
        bar.addItem(&top);
        // The Window menu is where AppKit lists open windows and where it
        // puts the tick beside the front one; telling it which menu that is
        // costs one call and is what makes the menu a Window menu rather than
        // a menu titled Window.
        if m.title == "Window" {
            app.setWindowsMenu(Some(&sub));
        }
    }

    app.setMainMenu(Some(&bar));
    TARGET.with(|t| *t.borrow_mut() = Some(target));
    true
}

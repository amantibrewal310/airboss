//! The only module with unsafe code: AppKit calls `objc2` can't check, and the quick-add
//! panel's class.
//!
//! - the system accent colour, and when it changes (`NSSystemColorsDidChangeNotification`)
//! - the Mac waking (`NSWorkspaceDidWakeNotification`)
//! - the local time zone's offset, for the menu-bar menu's clock times
//! - `QuickAddPanel`, an `NSPanel` subclass from `tauri-nspanel`

#![allow(unsafe_code)]

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_app_kit::{
    NSColor, NSColorSpace, NSSystemColorsDidChangeNotification, NSWorkspace,
    NSWorkspaceDidWakeNotification,
};
use objc2_foundation::{
    NSDate, NSNotification, NSNotificationCenter, NSObjectProtocol, NSString, NSTimeZone,
    NSUserDefaults,
};

/// The quick-add panel's class: a floating panel that can become key (to take typing) but
/// never main, so showing it doesn't bring the main window forward.
mod panel {
    // The macro expands `use` items and `define_class!` (which needs unsafe) in place.
    tauri_nspanel::tauri_panel! {
        panel!(QuickAddPanel {
            config: {
                can_become_key_window: true,
                can_become_main_window: false,
                is_floating_panel: true
            }
        })
    }
}
pub use panel::QuickAddPanel;

/// The system accent as `#rrggbb`, or `None` when the user chose Multicolor (then the app
/// keeps its own indigo everywhere).
pub fn accent_hex() -> Option<String> {
    // Multicolor is the absence of AppleAccentColor; any chosen colour, blue included, sets it.
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.objectForKey(&NSString::from_str("AppleAccentColor"))?;
    let color =
        NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        c(color.redComponent()),
        c(color.greenComponent()),
        c(color.blueComponent())
    ))
}

/// The local time zone's offset from UTC at `secs` since the epoch, in seconds.
pub fn local_offset(secs: f64) -> i64 {
    let date = NSDate::dateWithTimeIntervalSince1970(secs);
    NSTimeZone::localTimeZone().secondsFromGMTForDate(&date) as i64
}

/// What the observer calls.
pub struct Callbacks {
    pub woke: Box<dyn Fn()>,
    pub colors_changed: Box<dyn Fn()>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Observer` doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "AirbossObserver"]
    #[ivars = Callbacks]
    struct Observer;

    impl Observer {
        // SAFETY: the selector takes one object argument and returns void, as
        // `addObserver:selector:name:object:` requires.
        #[unsafe(method(woke:))]
        fn woke(&self, _note: &NSNotification) {
            (self.ivars().woke)();
        }

        // SAFETY: as above.
        #[unsafe(method(colorsChanged:))]
        fn colors_changed(&self, _note: &NSNotification) {
            (self.ivars().colors_changed)();
        }
    }

    unsafe impl NSObjectProtocol for Observer {}
);

impl Observer {
    fn new(callbacks: Callbacks) -> Retained<Observer> {
        let this = Observer::alloc().set_ivars(callbacks);
        // SAFETY: NSObject's designated initialiser, called once on a fresh allocation.
        unsafe { msg_send![super(this), init] }
    }
}

/// Starts listening for wake and accent changes for the app's lifetime. Call it once, on the
/// main thread (notifications then arrive there).
pub fn observe(callbacks: Callbacks) {
    let observer = Observer::new(callbacks);
    let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the observer implements both selectors with the notification signature, and it
    // is never deallocated (leaked below), so the centres never message a freed object. The
    // notification names are AppKit's own constants, valid for the process's life.
    unsafe {
        workspace.addObserver_selector_name_object(
            &observer,
            sel!(woke:),
            Some(NSWorkspaceDidWakeNotification),
            None,
        );
        center.addObserver_selector_name_object(
            &observer,
            sel!(colorsChanged:),
            Some(NSSystemColorsDidChangeNotification),
            None,
        );
    }
    // Lives as long as the app: the notification centres hold it weakly.
    std::mem::forget(observer);
}

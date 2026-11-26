//! The only module with unsafe code: AppKit calls `objc2` can't check, and the quick-add
//! panel's class.
//!
//! - the system accent colour, and when it changes (`NSSystemColorsDidChangeNotification`)
//! - the Mac waking (`NSWorkspaceDidWakeNotification`)
//! - the local time zone's offset, for the menu-bar menu's clock times
//! - the menu-bar item's accessibility label
//! - `QuickAddPanel`, an `NSPanel` subclass from `tauri-nspanel`
//! - notifications through `UNUserNotificationCenter`: permission, posting, withdrawing, and
//!   the delegate that hears a click
//! - the app that opens a URL (`NSWorkspace`), and whether an app's code signature meets a
//!   requirement (the Security framework's static code checks)

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
use objc2_user_notifications::{
    UNNotification, UNNotificationPresentationOptions, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
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

/// Turns off AppKit's window restoration for this app. Airboss restores its window's size
/// and position itself (window-state), and an unclean quit would otherwise make the next
/// launch stop on a modal "reopen windows?" alert before any window shows.
pub fn disable_state_restoration() {
    NSUserDefaults::standardUserDefaults()
        .setBool_forKey(true, &NSString::from_str("ApplePersistenceIgnoreState"));
}

/// Sets the menu-bar item's accessibility label (what VoiceOver reads for its button). On
/// the main thread only; elsewhere it does nothing.
pub fn set_status_item_label(item: &objc2_app_kit::NSStatusItem, label: &str) {
    use objc2_app_kit::NSAccessibility;
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };
    if let Some(button) = item.button(mtm) {
        button.setAccessibilityLabel(Some(&NSString::from_str(label)));
    }
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

/// What macOS answered about notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyAuth {
    Granted,
    Denied,
    NotAsked,
}

fn auth(status: objc2_user_notifications::UNAuthorizationStatus) -> NotifyAuth {
    use objc2_user_notifications::UNAuthorizationStatus as S;
    if status == S::Denied {
        NotifyAuth::Denied
    } else if status == S::NotDetermined {
        NotifyAuth::NotAsked
    } else {
        NotifyAuth::Granted
    }
}

/// Whether this process has a bundle identifier, which `UNUserNotificationCenter` needs: in a
/// process without one (a dev build run from `target/`) it raises an exception, which would
/// abort the app. Every notification call below checks this first.
pub fn has_bundle_id() -> bool {
    objc2_foundation::NSBundle::mainBundle()
        .bundleIdentifier()
        .is_some_and(|id| id.length() > 0)
}

/// Whether Airboss may notify, as System Settings › Notifications says; `None` if macOS
/// didn't answer within `wait`, or there's no bundle identifier.
pub fn notify_permission(wait: std::time::Duration) -> Option<NotifyAuth> {
    use objc2_user_notifications::UNNotificationSettings;
    if !has_bundle_id() {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(Some(tx));
    let block = block2::RcBlock::new(move |settings: std::ptr::NonNull<UNNotificationSettings>| {
        // SAFETY: the centre calls this once with a valid settings object it keeps alive
        // for the call.
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(auth(status));
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .getNotificationSettingsWithCompletionHandler(&block);
    rx.recv_timeout(wait).ok()
}

/// Asks macOS for permission to notify (banners and sounds). macOS shows its prompt only
/// the first time; after that it answers from the setting. Nothing without a bundle
/// identifier.
pub fn request_notify_permission() {
    use objc2::runtime::Bool;
    use objc2_foundation::NSError;
    use objc2_user_notifications::UNAuthorizationOptions;
    if !has_bundle_id() {
        return;
    }
    let block = block2::RcBlock::new(|granted: Bool, _error: *mut NSError| {
        tauri_plugin_log::log::info!("notifications allowed: {}", granted.as_bool());
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &block,
        );
}

/// Posts one notification. `id` is stable for one Needs me item, so posting it again replaces
/// it instead of adding another; `thread` groups a task's notifications together. Returns
/// once the request is handed to macOS; a delivery error is logged. An error without a
/// bundle identifier.
pub fn post_notification(id: &str, title: &str, body: &str, thread: &str) -> Result<(), String> {
    use objc2_foundation::NSError;
    use objc2_user_notifications::{
        UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound,
    };
    if !has_bundle_id() {
        return Err("there's no bundle identifier (a development build)".into());
    }
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setThreadIdentifier(&NSString::from_str(thread));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(id),
        &content,
        None,
    );
    let done = block2::RcBlock::new(|error: *mut NSError| {
        // SAFETY: the centre passes either null or an error it keeps alive for the call.
        if let Some(e) = unsafe { error.as_ref() } {
            tauri_plugin_log::log::warn!(
                "macOS didn't take a notification: {}",
                e.localizedDescription()
            );
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&done));
    Ok(())
}

/// Removes the notifications with these identifiers (the ones [`post_notification`] used)
/// from Notification Center, and any not delivered yet. An identifier with no notification is
/// skipped. Nothing without a bundle identifier.
pub fn withdraw_notifications(ids: &[&str]) {
    use objc2_foundation::NSArray;
    if !has_bundle_id() || ids.is_empty() {
        return;
    }
    let ids: Vec<Retained<NSString>> = ids.iter().map(|id| NSString::from_str(id)).collect();
    let ids = NSArray::from_retained_slice(&ids);
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.removeDeliveredNotificationsWithIdentifiers(&ids);
    center.removePendingNotificationRequestsWithIdentifiers(&ids);
}

/// What the notification delegate calls with a clicked notification's identifier.
pub struct Clicked(pub Box<dyn Fn(String) + Send + Sync>);

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `NotifyDelegate` doesn't implement
    // Drop.
    #[unsafe(super(NSObject))]
    #[name = "AirbossNotificationDelegate"]
    #[ivars = Clicked]
    struct NotifyDelegate;

    unsafe impl NSObjectProtocol for NotifyDelegate {}

    // SAFETY: both methods match the protocol's selectors and signatures, and each calls its
    // completion handler exactly once.
    unsafe impl UNUserNotificationCenterDelegate for NotifyDelegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            (self.ivars().0)(id);
            completion.call(());
        }

        // A notification that arrives while Airboss is the active app (with quick add or
        // Settings in front) still shows: the main window's focus already kept it from being
        // posted at all.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            use UNNotificationPresentationOptions as O;
            completion.call((O::Banner | O::List | O::Sound,));
        }
    }
);

/// Makes Airboss the notification centre's delegate, so a click reaches `clicked`. Call it
/// once, on the main thread, early at launch, so a click that launched the app is heard too.
/// Nothing without a bundle identifier.
pub fn set_notification_delegate(clicked: Clicked) {
    use objc2::runtime::ProtocolObject;
    if !has_bundle_id() {
        return;
    }
    let this = NotifyDelegate::alloc().set_ivars(clicked);
    // SAFETY: NSObject's designated initialiser, called once on a fresh allocation.
    let delegate: Retained<NotifyDelegate> = unsafe { msg_send![super(this), init] };
    let as_delegate: &ProtocolObject<dyn UNUserNotificationCenterDelegate> =
        ProtocolObject::from_ref(&*delegate);
    UNUserNotificationCenter::currentNotificationCenter().setDelegate(Some(as_delegate));
    // The centre holds its delegate weakly; this one lives as long as the app.
    std::mem::forget(delegate);
}

/// The app LaunchServices would open `url` with, as a path; `None` if there's none.
pub fn handler_for_url(url: &str) -> Option<std::path::PathBuf> {
    let url = objc2_foundation::NSURL::URLWithString(&NSString::from_str(url))?;
    let app = NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url)?;
    Some(std::path::PathBuf::from(app.path()?.to_string()))
}

/// Whether the code at `app` (a bundle) is validly signed, its resources sealed and unchanged,
/// and meets the code requirement `requirement` (`codesign -R` syntax). False for unsigned,
/// ad-hoc signed or modified code, or a requirement that doesn't parse.
pub fn code_satisfies(app: &std::path::Path, requirement: &str) -> bool {
    use std::ptr::{NonNull, null, null_mut};

    use objc2_core_foundation::{CFRetained, CFString, CFURL};
    use objc2_security::{SecCSFlags, SecRequirement, SecStaticCode};

    const ERR_SEC_SUCCESS: i32 = 0;
    // kSecCSStrictValidate | kSecCSCheckNestedCode: the stricter checks `codesign --verify`
    // makes, and the frameworks inside the bundle too.
    let validate = SecCSFlags(1 << 4 | 1 << 3);
    let Some(url) = CFURL::from_directory_path(app) else {
        return false;
    };
    let mut code: *const SecStaticCode = null();
    // SAFETY: `url` is a valid CFURL and `code` a valid out-pointer for the call.
    let made =
        unsafe { SecStaticCode::create_with_path(&url, SecCSFlags(0), NonNull::from(&mut code)) };
    let Some(code) = NonNull::new(code.cast_mut()).filter(|_| made == ERR_SEC_SUCCESS) else {
        return false;
    };
    // SAFETY: a successful create returns a +1 reference, which `CFRetained` now owns.
    let code: CFRetained<SecStaticCode> = unsafe { CFRetained::from_raw(code) };
    let text = CFString::from_str(requirement);
    let mut req: *mut SecRequirement = null_mut();
    // SAFETY: `text` is a valid CFString and `req` a valid out-pointer for the call.
    let parsed = unsafe {
        SecRequirement::create_with_string(&text, SecCSFlags(0), NonNull::from(&mut req))
    };
    let Some(req) = NonNull::new(req).filter(|_| parsed == ERR_SEC_SUCCESS) else {
        return false;
    };
    // SAFETY: as above, a +1 reference.
    let req: CFRetained<SecRequirement> = unsafe { CFRetained::from_raw(req) };
    // SAFETY: both are valid, owned references for the call.
    unsafe { code.check_validity(validate, Some(&req)) == ERR_SEC_SUCCESS }
}

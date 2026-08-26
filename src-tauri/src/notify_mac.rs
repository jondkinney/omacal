//! macOS: the real notification centre, buttons and click included.
//!
//! Until v0.5.0 this file could not exist: `UNUserNotificationCenter` refuses
//! everything outside a correctly signed bundle, and the design's answer
//! (spec §2.4) was to ship best-effort banners through
//! `tauri-plugin-notification` and be honest about it. The signed, notarized
//! `.dmg` dissolved the premise, so macOS now gets what Omarchy has had all
//! along: Join and Snooze as real buttons, and a click that lands the app on
//! the occurrence — through the same [`Action`] dispatch, resolved by the
//! same tested table (`action_for_un` normalises Apple's identifiers into
//! `action_for_key`'s keys).
//!
//! Two platform shapes worth naming, because they drove the design:
//!
//! **Buttons are declared up front, not per post.** The centre knows
//! categories, registered once; a notification only names one. So the button
//! layouts live in `notify::un_category`'s table and are registered in
//! [`UnNotifier::new`].
//!
//! **A click carries nothing but two strings back** — the request identifier
//! and the action identifier. There is no D-Bus-style payload riding the
//! signal, so the actions themselves are serialized *into* the request
//! identifier at post (`notify::un_payload`) and parsed back out in the
//! delegate. That also buys replacement-for-free: a re-fired reminder for the
//! same occurrence replaces its stale predecessor instead of stacking.
//!
//! What this deliberately does not do: sticky. The invitation toast's
//! stay-until-answered urgency is a D-Bus concept; macOS decides banner
//! versus persistent alert per app in System Settings, and fighting that
//! would need the time-sensitive entitlement for a behaviour the user can
//! set themselves. The invite still arrives, still accepts on click, and
//! still has the in-app tray behind it — the inbox was always the backstop
//! for a missed toast.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use crate::notify::{
    action_for_un, un_category, un_payload, Action, Notification, Notifier, NotifyError,
    UN_CATEGORY_JOIN, UN_CATEGORY_REMINDER,
};

/// What the delegate needs at click time: the dispatcher, the same `Arc` the
/// D-Bus notifier is handed on Linux.
struct Ivars {
    on_action: Arc<dyn Fn(Action) + Send + Sync>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this class adds
    // no Drop. Named explicitly because a delegate class shows up in crash
    // logs and "OmacalNotificationDelegate" says whose it is.
    #[unsafe(super(NSObject))]
    #[name = "OmacalNotificationDelegate"]
    #[ivars = Ivars]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        // The click path. Whichever thread the centre calls this on, the
        // dispatcher only spawns, emits and proxies — all Send-safe by
        // design (`dispatch_notification_action`).
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let key = response.actionIdentifier().to_string();
            let payload = response.notification().request().identifier().to_string();
            // `None` is a dismissal, a button this notification never
            // offered, or an identifier some older build wrote — silence
            // for all three, exactly as the D-Bus path treats `__closed`.
            if let Some(action) = action_for_un(&payload, &key) {
                (self.ivars().on_action)(action);
            }
            completion_handler.call(());
        }

        // Without this, macOS suppresses notifications while the app is
        // frontmost — but a reminder firing while the calendar is open is
        // still a reminder, and mako shows it regardless of focus. Banner
        // plus List so it also lands in Notification Center's history the
        // way a background delivery would.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler.call((
                UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,
            ));
        }
    }
);

impl NotificationDelegate {
    fn new(on_action: Arc<dyn Fn(Action) + Send + Sync>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { on_action });
        // SAFETY: plain NSObject init on an allocated, ivar-initialised self.
        unsafe { msg_send![super(this), init] }
    }
}

/// The signed-bundle transport. Holds no Objective-C object — see [`Self::new`]
/// for where each one went — so `Send + Sync` is the compiler's conclusion
/// rather than an unsafe promise, which is what [`Notifier`] requires of it.
pub(crate) struct UnNotifier {
    /// Post counter, nonce for `un_payload` — what keeps two same-shaped
    /// announcements from replacing each other in Notification Center.
    posts: AtomicU64,
}

impl UnNotifier {
    /// `None` when the process runs unbundled — `cargo tauri dev`, or the
    /// raw binary from a terminal — where `UNUserNotificationCenter` raises
    /// an Objective-C exception rather than returning. The caller keeps the
    /// legacy plugin path for that run, so a dev build behaves exactly as
    /// every build did before this file existed.
    pub(crate) fn new(on_action: Arc<dyn Fn(Action) + Send + Sync>) -> Option<Self> {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            tracing::info!(
                "running unbundled; notifications stay on the legacy best-effort path"
            );
            return None;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();

        // The permission prompt, on first launch of a build that has never
        // asked; every later launch answers from the recorded grant without
        // showing anything. Fire-and-forget deliberately: a user who clicks
        // "Don't Allow" has answered, and the scheduler keeps recording
        // fired reminders identically either way (§2.4's degrade-quietly
        // rule did not move, only the odds did).
        let asked = RcBlock::new(|granted: Bool, error: *mut NSError| {
            if !error.is_null() {
                // SAFETY: the centre passes a valid NSError or NULL, valid
                // for the duration of this call.
                let why = unsafe { &*error }.localizedDescription().to_string();
                tracing::warn!(error = %why, "notification authorization failed");
            } else if !granted.as_bool() {
                tracing::info!("notifications declined in the permission prompt");
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &asked,
        );

        // The button layouts, registered once — `notify::un_category` picks
        // per notification. Join deliberately not `Foreground`: the handler
        // opens the meeting link itself, and yanking the whole app forward
        // beside the browser would be the opposite of "one click, in the
        // call".
        let join = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str("join"),
            &NSString::from_str("Join"),
            UNNotificationActionOptions::empty(),
        );
        let snooze = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str("snooze"),
            &NSString::from_str("Snooze 5m"),
            UNNotificationActionOptions::empty(),
        );
        let no_intents: Retained<NSArray<NSString>> = NSArray::from_retained_slice(&[]);
        let with_join = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &NSString::from_str(UN_CATEGORY_JOIN),
            &NSArray::from_retained_slice(&[join, snooze.clone()]),
            &no_intents,
            UNNotificationCategoryOptions::empty(),
        );
        let plain = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &NSString::from_str(UN_CATEGORY_REMINDER),
            &NSArray::from_retained_slice(&[snooze]),
            &no_intents,
            UNNotificationCategoryOptions::empty(),
        );
        center.setNotificationCategories(&NSSet::from_retained_slice(&[with_join, plain]));

        // The delegate — the centre holds it *weak*, so the forget below is
        // its retention: one app-lifetime object, leaked on purpose, exactly
        // as long-lived as the centre that calls into it. Storing it here
        // instead would drag `!Send` Objective-C references into a struct
        // the scheduler shares across threads.
        let delegate = NotificationDelegate::new(on_action);
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        std::mem::forget(delegate);

        Some(Self { posts: AtomicU64::new(0) })
    }
}

impl Notifier for UnNotifier {
    fn post(&self, n: &Notification) -> Result<(), NotifyError> {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&n.title));
        content.setBody(&NSString::from_str(&n.body));
        let category = un_category(&n.actions);
        if !category.is_empty() {
            content.setCategoryIdentifier(&NSString::from_str(category));
        }
        // The actions ride the identifier — the one string the click returns.
        let identifier = un_payload(self.posts.fetch_add(1, Ordering::Relaxed), &n.actions);
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&identifier),
            &content,
            // No trigger: a reminder's timing was the scheduler's decision,
            // already made — this is delivery, not scheduling.
            None,
        );

        // Delivery is asynchronous and its verdict arrives after `post` has
        // returned, so a refusal is logged rather than reported — the same
        // trade the driver already makes: the reminder is recorded as fired
        // whatever the transport managed (§2.4).
        let done = RcBlock::new(|error: *mut NSError| {
            if !error.is_null() {
                // SAFETY: valid NSError or NULL, for the duration of the call.
                let why = unsafe { &*error }.localizedDescription().to_string();
                tracing::warn!(error = %why, "the notification centre refused a post");
            }
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&done));
        Ok(())
    }
}

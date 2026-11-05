//! Drag-to-grant helper for Accessibility. A small panel docks under System Settings and holds the
//! app icon. Dropping the icon on the permission list adds Gapir there already switched on, so the
//! user does not have to find it in a long list.

use crate::permissions;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSControl, NSDragOperation, NSDraggingContext, NSDraggingItem,
    NSDraggingSession, NSDraggingSource, NSEvent, NSFloatingWindowLevel, NSFont, NSImageView,
    NSPanel, NSResponder, NSScreen, NSTextField, NSView, NSWindowStyleMask,
    NSWindowTitleVisibility, NSWorkspace,
};
use objc2_core_foundation::CFArray;
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{
    ns_string, NSArray, NSBundle, NSDictionary, NSNumber, NSObject, NSPoint, NSRect, NSSize,
    NSString, NSURL,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::AppHandle;

const SETTINGS_APP: &str = "com.apple.systempreferences";
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 84.0;
const ICON: f64 = 56.0;
const GAP: f64 = 12.0;
const POLL: Duration = Duration::from_millis(300);
/// The helper never lingers, even if the user walks away from System Settings.
const GIVE_UP: Duration = Duration::from_secs(10 * 60);

static ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Touched on the main thread only.
    static PANEL: RefCell<Option<Retained<NSPanel>>> = const { RefCell::new(None) };
}

define_class!(
    /// The app icon. Pressing it starts a drag of the app bundle.
    #[unsafe(super(NSImageView, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GapirGrantIcon"]
    struct GrantIcon;

    impl GrantIcon {
        /// The panel never becomes key, so the first click must already start the drag.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let url = NSURL::fileURLWithPath(&NSBundle::mainBundle().bundlePath());
            let item = NSDraggingItem::initWithPasteboardWriter(
                NSDraggingItem::alloc(),
                ProtocolObject::from_ref(&*url),
            );
            let image = self.image();
            // SAFETY: the contents argument takes an image, as documented for NSDraggingItem.
            unsafe {
                item.setDraggingFrame_contents(
                    self.bounds(),
                    image.as_deref().map(|image| image.as_ref()),
                );
            }
            self.beginDraggingSessionWithItems_event_source(
                &NSArray::from_retained_slice(&[item]),
                event,
                ProtocolObject::from_ref(self),
            );
        }
    }

    unsafe impl NSObjectProtocol for GrantIcon {}

    unsafe impl NSDraggingSource for GrantIcon {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }
    }
);

impl GrantIcon {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        // SAFETY: `initWithFrame:` is the designated initializer of NSImageView.
        let icon: Retained<Self> = unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] };
        let path = NSBundle::mainBundle().bundlePath();
        icon.setImage(Some(&NSWorkspace::sharedWorkspace().iconForFile(&path)));
        icon
    }
}

/// Shows the helper next to System Settings until Accessibility is granted.
pub fn show(app: &AppHandle) {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        while !permissions::accessibility() && started.elapsed() < GIVE_UP {
            let _ = app.run_on_main_thread(follow_settings);
            std::thread::sleep(POLL);
        }
        let _ = app.run_on_main_thread(close);
        ACTIVE.store(false, Ordering::SeqCst);
    });
}

fn follow_settings() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let window = settings_window();
    PANEL.with_borrow_mut(|slot| match window {
        Some(window) => {
            let panel = slot.get_or_insert_with(|| build(mtm));
            panel.setFrameOrigin(origin(window, mtm));
            panel.orderFrontRegardless();
        }
        None => {
            if let Some(panel) = slot {
                panel.orderOut(None);
            }
        }
    });
}

fn close() {
    PANEL.with_borrow_mut(|slot| {
        if let Some(panel) = slot.take() {
            panel.close();
        }
    });
}

/// Frame of the front System Settings window in top-left screen coordinates, while it is the
/// active app. Window metadata needs no permission; only window images do.
fn settings_window() -> Option<NSRect> {
    let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    if front.bundleIdentifier()?.to_string() != SETTINGS_APP {
        return None;
    }
    let pid = front.processIdentifier();
    let list = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    )?;
    // SAFETY: a CFArray of CFDictionary is toll-free bridged to NSArray of NSDictionary.
    let list: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(&*list as *const CFArray).cast() };
    let number = |info: &NSDictionary<NSString, AnyObject>, key: &NSString| {
        info.objectForKey(key)?
            .downcast::<NSNumber>()
            .ok()
            .map(|n| n.as_f64())
    };
    list.iter().find_map(|info| {
        if number(&info, ns_string!("kCGWindowOwnerPID"))? as i32 != pid
            || number(&info, ns_string!("kCGWindowLayer"))? != 0.0
        {
            return None;
        }
        let bounds = info
            .objectForKey(ns_string!("kCGWindowBounds"))?
            .downcast::<NSDictionary>()
            .ok()?;
        // SAFETY: the bounds dictionary maps NSString keys to NSNumber values.
        let bounds: &NSDictionary<NSString, AnyObject> =
            unsafe { &*(&*bounds as *const NSDictionary).cast() };
        Some(NSRect::new(
            NSPoint::new(
                number(bounds, ns_string!("X"))?,
                number(bounds, ns_string!("Y"))?,
            ),
            NSSize::new(
                number(bounds, ns_string!("Width"))?,
                number(bounds, ns_string!("Height"))?,
            ),
        ))
    })
}

/// Places the panel centred under the Settings window, or inside its bottom edge when the
/// screen ends first.
fn origin(window: NSRect, mtm: MainThreadMarker) -> NSPoint {
    let screens = NSScreen::screens(mtm);
    // Top-left coordinates count from the top of the primary screen.
    let primary_top = screens.firstObject().map_or(0.0, |s| s.frame().size.height);
    let x = window.origin.x + (window.size.width - WIDTH) / 2.0;
    let bottom = primary_top - (window.origin.y + window.size.height);
    let floor = screens
        .iter()
        .map(|s| s.visibleFrame())
        .find(|f| x + WIDTH / 2.0 >= f.origin.x && x + WIDTH / 2.0 < f.origin.x + f.size.width)
        .map_or(0.0, |f| f.origin.y);
    let below = bottom - GAP - HEIGHT;
    NSPoint::new(x, if below >= floor { below } else { bottom + GAP })
}

fn build(mtm: MainThreadMarker) -> Retained<NSPanel> {
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::FullSizeContentView
        | NSWindowStyleMask::NonactivatingPanel;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, HEIGHT)),
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    // SAFETY: the panel is owned by `PANEL`, so AppKit must not release it on close.
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setTitlebarAppearsTransparent(true);
    panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    panel.setFloatingPanel(true);
    panel.setLevel(NSFloatingWindowLevel);
    // Gapir is never the active app here: System Settings is.
    panel.setHidesOnDeactivate(false);
    panel.setBecomesKeyOnlyIfNeeded(true);

    let content = panel.contentView().expect("panel has a content view");
    let icon = GrantIcon::new(
        mtm,
        NSRect::new(
            NSPoint::new(16.0, (HEIGHT - ICON) / 2.0),
            NSSize::new(ICON, ICON),
        ),
    );
    content.addSubview(&icon);

    let left = 16.0 + ICON + 14.0;
    let title = NSTextField::labelWithString(ns_string!("Gapirʼni roʻyxatga sudrang"), mtm);
    title.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    title.setFrame(NSRect::new(
        NSPoint::new(left, 46.0),
        NSSize::new(WIDTH - left - 12.0, 18.0),
    ));
    content.addSubview(&title);

    let hint = NSTextField::wrappingLabelWithString(
        ns_string!("Belgini ushlab, yuqoridagi roʻyxatga tashlang. Ruxsat oʻzi yoqiladi."),
        mtm,
    );
    hint.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    hint.setTextColor(Some(&NSColor::secondaryLabelColor()));
    hint.setFrame(NSRect::new(
        NSPoint::new(left, 12.0),
        NSSize::new(WIDTH - left - 12.0, 32.0),
    ));
    content.addSubview(&hint);
    panel
}

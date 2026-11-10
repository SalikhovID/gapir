//! Drag-to-grant helper for Accessibility. A small panel docks under System Settings and holds the
//! app icon. Dropping the icon on the permission list adds Gapir there already switched on, so the
//! user does not have to find it in a long list.

use crate::{lifecycle, permissions};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSApplication, NSBackingStoreType, NSColor,
    NSControl, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSFloatingWindowLevel, NSFont, NSImage, NSImageScaling, NSImageView,
    NSPanel, NSResponder, NSScreen, NSTextField, NSView, NSWindowStyleMask,
    NSWindowTitleVisibility, NSWorkspace,
};
use objc2_application_services::AXUIElement;
use objc2_core_foundation::{CFArray, CFString, kCFBooleanTrue};
use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
use objc2_foundation::{
    NSArray, NSBundle, NSDictionary, NSNumber, NSObject, NSPoint, NSRect, NSSize, NSString, NSURL,
    ns_string,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::AppHandle;

const SETTINGS_APP: &str = "com.apple.systempreferences";
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 96.0;
const ICON: f64 = 64.0;
const ICON_X: f64 = 18.0;
const ICON_Y: f64 = (HEIGHT - ICON) / 2.0;
/// How far the icon bobs up, pointing at the list above.
const BOB: f64 = 7.0;
const GAP: f64 = 12.0;
/// Also the length of one bob, so the motion never stops between ticks.
const POLL: Duration = Duration::from_millis(450);
const SUCCESS_TIME: Duration = Duration::from_millis(1_300);
const FADE: f64 = 0.3;
/// The helper never lingers, even if the user walks away from System Settings.
const GIVE_UP: Duration = Duration::from_secs(10 * 60);

static ACTIVE: AtomicBool = AtomicBool::new(false);

struct Helper {
    panel: Retained<NSPanel>,
    icon: Retained<GrantIcon>,
    title: Retained<NSTextField>,
    hint: Retained<NSTextField>,
    /// Where the panel was last placed, to tell a moved Settings window from a steady one.
    origin: Option<NSPoint>,
    bob_up: bool,
}

thread_local! {
    /// Touched on the main thread only.
    static HELPER: RefCell<Option<Helper>> = const { RefCell::new(None) };
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
        icon.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        icon
    }
}

/// Shows the helper next to System Settings until Accessibility is granted, then celebrates and
/// brings the Gapir window back.
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
        if permissions::accessibility() {
            let _ = app.run_on_main_thread(celebrate);
            std::thread::sleep(SUCCESS_TIME);
            let _ = app.run_on_main_thread(fade_out);
            std::thread::sleep(Duration::from_secs_f64(FADE));
            let _ = app.run_on_main_thread(close);
            lifecycle::show_main(&app);
            let _ = app.run_on_main_thread(take_focus);
        } else {
            let _ = app.run_on_main_thread(close);
        }
        ACTIVE.store(false, Ordering::SeqCst);
    });
}

fn animate(duration: f64, changes: impl Fn() + 'static) {
    let group = RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
        // SAFETY: AppKit passes the live animation context for this group.
        unsafe { context.as_ref() }.setDuration(duration);
        changes();
    });
    NSAnimationContext::runAnimationGroup_completionHandler(&group, None);
}

fn follow_settings() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let window = settings_window();
    HELPER.with_borrow_mut(|slot| {
        let Some(window) = window else {
            if let Some(helper) = slot {
                helper.panel.orderOut(None);
                helper.origin = None;
            }
            return;
        };
        let helper = slot.get_or_insert_with(|| build(mtm));
        let target = origin(window, mtm);
        match helper.origin {
            // Appears: slides up and fades in.
            None => {
                let panel = helper.panel.clone();
                panel.setAlphaValue(0.0);
                panel.setFrameOrigin(NSPoint::new(target.x, target.y - 24.0));
                panel.orderFrontRegardless();
                animate(0.35, move || {
                    let proxy = panel.animator();
                    proxy.setAlphaValue(1.0);
                    proxy.setFrame_display(NSRect::new(target, panel.frame().size), true);
                });
            }
            // Settings moved: follow it at once.
            Some(placed) if placed != target => helper.panel.setFrameOrigin(target),
            Some(_) => {}
        }
        helper.origin = Some(target);

        helper.bob_up = !helper.bob_up;
        let icon = helper.icon.clone();
        let y = if helper.bob_up { ICON_Y + BOB } else { ICON_Y };
        animate(POLL.as_secs_f64(), move || {
            icon.animator().setFrameOrigin(NSPoint::new(ICON_X, y));
        });
    });
}

/// Swaps the app icon for a green check that pops in, so the grant is plain to see.
fn celebrate() {
    HELPER.with_borrow_mut(|slot| {
        let Some(helper) = slot else {
            return;
        };
        if helper.origin.is_none() {
            return;
        }
        if let Some(check) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            ns_string!("checkmark.circle.fill"),
            None,
        ) {
            helper.icon.setImage(Some(&check));
            helper
                .icon
                .setContentTintColor(Some(&NSColor::systemGreenColor()));
        }
        helper.title.setStringValue(ns_string!("Ruxsat berildi"));
        helper
            .hint
            .setStringValue(ns_string!("Gapir oynasiga qaytamiz."));
        let icon = helper.icon.clone();
        let full = NSRect::new(NSPoint::new(ICON_X, ICON_Y), NSSize::new(ICON, ICON));
        let small = 20.0;
        icon.setFrame(NSRect::new(
            NSPoint::new(ICON_X + small / 2.0, ICON_Y + small / 2.0),
            NSSize::new(ICON - small, ICON - small),
        ));
        animate(0.35, move || icon.animator().setFrame(full));
    });
}

fn fade_out() {
    HELPER.with_borrow(|slot| {
        let Some(helper) = slot else {
            return;
        };
        let panel = helper.panel.clone();
        animate(FADE, move || {
            let frame = panel.frame();
            let proxy = panel.animator();
            proxy.setAlphaValue(0.0);
            proxy.setFrame_display(
                NSRect::new(
                    NSPoint::new(frame.origin.x, frame.origin.y - 16.0),
                    frame.size,
                ),
                true,
            );
        });
    });
}

fn close() {
    HELPER.with_borrow_mut(|slot| {
        if let Some(helper) = slot.take() {
            helper.panel.close();
        }
    });
}

/// System Settings is the active app and may refuse a plain activation request. Accessibility is
/// granted by now, so Gapir can raise itself the way the user would.
fn take_focus() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(mtm).activate();
    // SAFETY: our own process id, and a static CFBoolean value.
    unsafe {
        let me = AXUIElement::new_application(std::process::id() as i32);
        if let Some(yes) = kCFBooleanTrue {
            me.set_attribute_value(&CFString::from_static_str("AXFrontmost"), yes);
        }
    }
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

fn build(mtm: MainThreadMarker) -> Helper {
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
        NSRect::new(NSPoint::new(ICON_X, ICON_Y), NSSize::new(ICON, ICON)),
    );
    content.addSubview(&icon);

    let left = ICON_X + ICON + 16.0;
    let title =
        NSTextField::labelWithString(ns_string!("Gapirʼni yuqoridagi roʻyxatga sudrang"), mtm);
    title.setFont(Some(&NSFont::boldSystemFontOfSize(15.0)));
    title.setFrame(NSRect::new(
        NSPoint::new(left, 52.0),
        NSSize::new(WIDTH - left - 12.0, 22.0),
    ));
    content.addSubview(&title);

    let hint = NSTextField::wrappingLabelWithString(
        ns_string!("Belgini ushlab, roʻyxatga tashlang. Ruxsat oʻzi yoqiladi."),
        mtm,
    );
    hint.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    hint.setTextColor(Some(&NSColor::secondaryLabelColor()));
    hint.setFrame(NSRect::new(
        NSPoint::new(left, 16.0),
        NSSize::new(WIDTH - left - 12.0, 34.0),
    ));
    content.addSubview(&hint);
    Helper {
        panel,
        icon,
        title,
        hint,
        origin: None,
        bob_up: false,
    }
}

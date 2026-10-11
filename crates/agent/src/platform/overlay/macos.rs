//! macOS overlay windows via AppKit (objc2). Runs on the process main thread.
//!
//! Windows created here are borderless, top-most and click-through:
//!   * blank window — full-screen black (blanking a class)
//!   * hud window   — transparency banner strip + teacher messages

use super::{state, Update};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSFont, NSMenu,
    NSMenuItem, NSStatusBar, NSStatusItem, NSScreen, NSTextField, NSTextAlignment, NSWindow,
    NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{
    NSDefaultRunLoopMode, NSDate, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURL,
};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

// Menu-bar actions for the tray/status item. Instances live for the whole
// overlay loop (AppKit menu targets are unretained), on the main thread.
define_class!(
    #[unsafe(super(NSObject))]
    struct TrayTarget;

    impl TrayTarget {
        // "Open Console" menu item → open the management console in the
        // default browser.
        #[unsafe(method(openConsole:))]
        fn open_console(&self, _sender: Option<&AnyObject>) {
            let url = state::console_url();
            if url.is_empty() {
                tracing::warn!("tray: console URL not known yet (agent not connected)");
                return;
            }
            let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(&url)) else {
                tracing::warn!(%url, "tray: unparseable console URL");
                return;
            };
            let _ = NSWorkspace::sharedWorkspace().openURL(&nsurl);
        }
    }
);

struct Ui {
    school: String,
    banner_on: bool,
    blanked: bool,
    message: Option<(String, Instant)>,
    blank_win: Option<Retained<NSWindow>>,
    hud_win: Option<Retained<NSWindow>>,
    banner_label: Option<Retained<NSTextField>>,
    message_label: Option<Retained<NSTextField>>,
}

impl Ui {
    fn new() -> Self {
        Ui {
            school: String::new(),
            banner_on: false,
            blanked: false,
            message: None,
            blank_win: None,
            hud_win: None,
            banner_label: None,
            message_label: None,
        }
    }

    fn screen_size(mtm: MainThreadMarker) -> NSRect {
        NSScreen::mainScreen(mtm)
            .map(|s| s.frame())
            .unwrap_or(NSRect {
                origin: NSPoint::new(0.0, 0.0),
                size: NSSize::new(1440.0, 900.0),
            })
    }

    fn new_window(mtm: MainThreadMarker, frame: NSRect) -> Option<Retained<NSWindow>> {
        unsafe {
            let win = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            );
            win.setOpaque(true);
            win.setBackgroundColor(Some(&NSColor::blackColor()));
            win.setHasShadow(false);
            win.setIgnoresMouseEvents(true);
            win.setLevel(20_000); // NSScreenSaverWindowLevel + margin
            Some(win)
        }
    }

    fn ensure_blank_window(&mut self, mtm: MainThreadMarker) {
        if self.blank_win.is_some() {
            return;
        }
        let frame = Self::screen_size(mtm);
        match Self::new_window(mtm, frame) {
            Some(win) => {
                win.orderFrontRegardless();
                self.blank_win = Some(win);
            }
            None => tracing::error!("failed to create blank window"),
        }
    }

    fn ensure_hud(&mut self, mtm: MainThreadMarker) {
        if self.hud_win.is_some() {
            return;
        }
        let screen = Self::screen_size(mtm);
        let w = 760.0f64.min(screen.size.width - 40.0);
        let rect = NSRect {
            origin: NSPoint::new((screen.size.width - w) / 2.0, screen.size.height - 74.0),
            size: NSSize::new(w, 64.0),
        };
        let Some(win) = Self::new_window(mtm, rect) else {
            tracing::error!("failed to create hud window");
            return;
        };
        win.setBackgroundColor(Some(&NSColor::systemYellowColor()));

        let banner = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        banner.setTextColor(Some(&NSColor::blackColor()));
        banner.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
        banner.setFrame(NSRect {
            origin: NSPoint::new(10.0, 32.0),
            size: NSSize::new(w - 20.0, 20.0),
        });
        let msg = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        msg.setTextColor(Some(&NSColor::blackColor()));
        msg.setFont(Some(&NSFont::boldSystemFontOfSize(16.0)));
        msg.setAlignment(NSTextAlignment::Center);
        msg.setFrame(NSRect {
            origin: NSPoint::new(10.0, 4.0),
            size: NSSize::new(w - 20.0, 28.0),
        });
        if let Some(content) = win.contentView() {
            content.addSubview(&banner);
            content.addSubview(&msg);
        }
        self.banner_label = Some(banner);
        self.message_label = Some(msg);
        self.hud_win = Some(win);
    }

    fn render(&mut self, mtm: MainThreadMarker) {
        // Blank window visibility tracks blanked state.
        if self.blanked {
            self.ensure_blank_window(mtm);
            if let Some(w) = &self.blank_win {
                w.orderFrontRegardless();
            }
            state::BLANKED.store(true, std::sync::atomic::Ordering::Relaxed);
        } else {
            if let Some(w) = &self.blank_win {
                w.orderOut(None);
            }
            state::BLANKED.store(false, std::sync::atomic::Ordering::Relaxed);
        }

        // Message expiry.
        if let Some((_, until)) = &self.message {
            if Instant::now() >= *until {
                self.message = None;
            }
        }

        let show_banner = self.banner_on;
        let show_message = self.message.is_some();
        if show_banner || show_message {
            self.ensure_hud(mtm);
            if let (Some(win), Some(banner), Some(msg)) =
                (&self.hud_win, &self.banner_label, &self.message_label)
            {
                let banner_text = if show_banner {
                    format!(
                        "🛡 {} — this device is monitored for safeguarding",
                        if self.school.is_empty() { "EduGuard" } else { &self.school }
                    )
                } else {
                    String::new()
                };
                banner.setStringValue(&NSString::from_str(&banner_text));
                msg.setStringValue(&NSString::from_str(
                    &self.message.as_ref().map(|(t, _)| t.clone()).unwrap_or_default(),
                ));
                win.orderFrontRegardless();
            }
        } else if let Some(win) = &self.hud_win {
            win.orderOut(None);
        }
    }
}

/// The macOS "tray": a discreet shield item in the menu bar (title text —
/// no overlay windows, no notifications). Its menu offers Open Console.
/// Created once AppKit is up; the returned item must outlive the run loop.
fn setup_status_item(mtm: MainThreadMarker, target: &TrayTarget) -> Retained<NSStatusItem> {
    unsafe {
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        if let Some(btn) = item.button(mtm) {
            btn.setTitle(&NSString::from_str("🛡"));
            btn.setToolTip(Some(&NSString::from_str("EduGuard — safeguarding agent")));
        }

        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("EduGuard"));
        menu.setAutoenablesItems(false);
        let status = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("EduGuard Agent — Connected"),
            None,
            &NSString::from_str(""),
        );
        status.setEnabled(false);
        let open = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Open Console"),
            Some(sel!(openConsole:)),
            &NSString::from_str(""),
        );
        open.setTarget(Some(&*target));
        menu.addItem(&status);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&open);
        item.setMenu(Some(&menu));
        item
    }
}

pub fn run(rx: Receiver<Update>) {
    let mtm = match MainThreadMarker::new() {
        Some(m) => m,
        None => {
            tracing::error!("overlay must run on the macOS main thread");
            return;
        }
    };
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    // Tray icon: menu-bar shield + Open Console. `tray_target` must stay
    // alive for the whole loop (NSMenuItem.target is unretained).
    let tray_target: Retained<TrayTarget> = unsafe { msg_send![TrayTarget::alloc(), init] };
    let _tray_item = setup_status_item(mtm, &tray_target);
    let mut ui = Ui::new();
    loop {
        while let Ok(update) = rx.try_recv() {
            match update {
                Update::Blank(b) => ui.blanked = b,
                Update::Banner(text) => {
                    ui.banner_on = text.is_some();
                    if let Some(t) = text {
                        ui.school = t;
                    }
                }
                Update::Message { text, ttl_secs } => {
                    ui.message = Some((text, Instant::now() + Duration::from_secs(ttl_secs)));
                }
                Update::ClearMessage => ui.message = None,
                Update::Quit => return,
            }
        }
        ui.render(mtm);
        unsafe {
            let deadline = NSDate::dateWithTimeIntervalSinceNow(0.2);
            let _ = NSRunLoop::currentRunLoop().runMode_beforeDate(
                NSDefaultRunLoopMode,
                &deadline,
            );
        }
    }
}

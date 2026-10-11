//! Linux (X11) overlay via x11rb. Wayland has no portable way to draw
//! privileged overlays without a compositor-specific protocol; the agent logs
//! an explicit "unsupported on Wayland" warning instead of failing silently
//! (GOAL §5: feature-gating must be explicit).

use super::{state, Update};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, CreateGCAux, CreateWindowAux, WindowClass};

struct Wins {
    blank: u32,
    hud: u32,
    gc_blank: u32,
    gc_hud: u32,
    font: Option<u32>,
    white: u32,
    black: u32,
    w: u16,
    h: u16,
}

pub fn run(rx: Receiver<Update>) {
    if std::env::var("WAYLAND_DISPLAY").is_ok() && std::env::var("DISPLAY").is_err() {
        tracing::warn!("blank/banner overlay unsupported on Wayland (needs xdg-desktop-portal session agent); classroom blanking is disabled on this device");
        // Drain updates so callers don't block on a full channel.
        while rx.recv().is_ok() {}
        return;
    }
    let (conn, screen_num) = match x11rb::connect(None) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("no X display available for overlay: {e}");
            while rx.recv().is_ok() {}
            return;
        }
    };
    let screen = &conn.setup().roots[screen_num];
    let white = screen.white_pixel;
    let black = screen.black_pixel;
    let root = screen.root;

    // Core font for text; optional. OpenFont needs a caller-allocated id, and
    // a missing font is reported asynchronously on cookie.check().
    let mut font: Option<u32> = None;
    for name in [b"fixed".as_slice(), b"9x15".as_slice()] {
        let Ok(fid) = conn.generate_id() else { break };
        let Ok(cookie) = conn.open_font(fid, name) else { break };
        if cookie.check().is_ok() {
            font = Some(fid);
            break;
        }
    }

    let blank = conn.generate_id().unwrap();
    let hud = conn.generate_id().unwrap();
    let (w, h) = (screen.width_in_pixels, screen.height_in_pixels);

    let _ = conn
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            blank,
            root,
            0,
            0,
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .background_pixel(black)
                .event_mask(x11rb::protocol::xproto::EventMask::EXPOSURE),
        )
        .unwrap();
    let hud_w = 900u16.min(w.saturating_sub(20));
    let _ = conn
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            hud,
            root,
            ((w - hud_w) / 2) as i16,
            0,
            hud_w,
            60,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .background_pixel(white)
                .override_redirect(1u32)
                .event_mask(x11rb::protocol::xproto::EventMask::EXPOSURE),
        )
        .unwrap();

    let gc_blank = conn.generate_id().unwrap();
    conn.create_gc(gc_blank, blank, &CreateGCAux::new().foreground(black))
        .unwrap();
    let gc_hud = conn.generate_id().unwrap();
    let mut hud_aux = CreateGCAux::new().foreground(black).background(white);
    if let Some(f) = font {
        hud_aux = hud_aux.font(f);
    }
    conn.create_gc(gc_hud, hud, &hud_aux).unwrap();

    conn.map_window(blank).unwrap();
    conn.unmap_window(hud).unwrap();
    // Keep the blank window above its siblings.
    let _ = conn
        .change_window_attributes(blank, &x11rb::protocol::xproto::ChangeWindowAttributesAux::new().override_redirect(1u32));
    conn.flush().unwrap();

    let mut wins = Wins {
        blank,
        hud,
        gc_blank,
        gc_hud,
        font,
        white,
        black,
        w,
        h,
    };
    let mut blanked = false;
    let mut banner_on = false;
    let mut school = String::new();
    let mut message: Option<(String, Instant)> = None;
    let mut need_redraw = true;

    loop {
        while let Ok(update) = rx.try_recv() {
            match update {
                Update::Blank(b) => {
                    blanked = b;
                    state::BLANKED.store(b, std::sync::atomic::Ordering::Relaxed);
                    let _ = if b {
                        conn.map_window(blank)
                    } else {
                        conn.unmap_window(blank)
                    };
                    need_redraw = true;
                }
                Update::Banner(text) => {
                    banner_on = text.is_some();
                    if let Some(t) = text {
                        school = t;
                    }
                    need_redraw = true;
                }
                Update::Message { text, ttl_secs } => {
                    message = Some((text, Instant::now() + Duration::from_secs(ttl_secs)));
                    need_redraw = true;
                }
                Update::ClearMessage => {
                    message = None;
                    need_redraw = true;
                }
                Update::Quit => return,
            }
        }
        if let Some((_, until)) = &message {
            if Instant::now() >= *until {
                message = None;
                need_redraw = true;
            }
        }
        let hud_line = if banner_on || message.is_some() {
            let mut s = String::new();
            if banner_on {
                s.push_str(&format!(
                    "Monitored by {}",
                    if school.is_empty() { "EduGuard" } else { &school }
                ));
            }
            if let Some((m, _)) = &message {
                if !s.is_empty() {
                    s.push_str("  -  ");
                }
                s.push_str(m);
            }
            s
        } else {
            String::new()
        };

        if need_redraw {
            need_redraw = false;
            // Repaint blank canvas.
            let _ = conn.poly_fill_rectangle(blank, gc_blank, &[x11rb::protocol::xproto::Rectangle {
                x: 0,
                y: 0,
                width: wins.w,
                height: wins.h,
            }]);
            if hud_line.is_empty() {
                let _ = conn.unmap_window(hud);
            } else {
                let _ = conn.map_window(hud);
                let _ = conn.poly_fill_rectangle(
                    hud,
                    wins.gc_hud,
                    &[x11rb::protocol::xproto::Rectangle {
                        x: 0,
                        y: 0,
                        width: wins.w.min(900),
                        height: 60,
                    }]);
                if wins.font.is_some() {
                    let bytes = hud_line.as_bytes();
                    let len = bytes.len().min(110) as u8;
                    let _ = conn.image_text8(hud, wins.gc_hud, 10, 36, &bytes[..len as usize]);
                }
            }
            let _ = conn.flush();
        }

        // Drain X events (exposures etc.) without blocking.
        while let Ok(Some(_ev)) = conn.poll_for_event() {}
        std::thread::sleep(Duration::from_millis(150));
    }
}

//! Windows overlay: borderless top-most windows driven from a dedicated UI
//! thread with its own message pump (Win32, windows-sys only — links against
//! import libs shipped by the windows-targets crates, so it builds fine with
//! the GNU toolchain).

use super::{state, Update};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, ReleaseDC, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT, COLOR_WINDOW, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    HBITMAP, HBRUSH, HGDIOBJ, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, ShellExecuteW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DispatchMessageW, GetCursorPos, ICONINFO, MoveWindow, PeekMessageW,
    RegisterClassW, SetForegroundWindow, ShowWindow, TrackPopupMenu, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, HICON, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PM_REMOVE, SW_SHOW,
    TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_COMMAND, WM_LBUTTONUP, WM_PAINT,
    WM_QUIT, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

/// Tray-icon callback (legacy semantics: lParam = mouse message).
const WM_TRAYICON: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
/// Popup-menu command ids.
const CMD_OPEN_CONSOLE: usize = 1;

struct UiState {
    school: String,
    banner_on: bool,
    blanked: bool,
    message: Option<(String, Instant)>,
}

static mut BLANK_HWND: HWND = std::ptr::null_mut();
static mut HUD_HWND: HWND = std::ptr::null_mut();
static mut TRAY_ICON: HICON = std::ptr::null_mut();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Open the management console in the default browser (tray action).
fn open_console() {
    let url = state::console_url();
    if url.is_empty() {
        tracing::warn!("tray: console URL not known yet (agent not connected)");
        return;
    }
    unsafe {
        let op = wide("open");
        let file = wide(&url);
        let ret = ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // SW_SHOWNORMAL
        );
        if (ret as isize) <= 32 {
            tracing::warn!("tray: ShellExecuteW failed for {url}");
        }
    }
}

/// Draw a small EduGuard icon (blue rounded square, white "E") at runtime so
/// the agent needs no shipped .ico resource.
unsafe fn make_tray_icon() -> HICON {
    const S: i32 = 16;
    let screen = GetDC(std::ptr::null_mut());
    let mem = CreateCompatibleDC(screen);
    if screen.is_null() || mem.is_null() {
        if !mem.is_null() {
            DeleteDC(mem);
        }
        if !screen.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen);
        }
        return std::ptr::null_mut();
    }
    let full = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: S,
        bottom: S,
    };

    // Color layer: blue square + white "E".
    let color: HBITMAP = CreateCompatibleBitmap(screen, S, S);
    let old = SelectObject(mem, color as HGDIOBJ);
    let blue = CreateSolidBrush(0x0025_63eb); // COLORREF 0x00BBGGRR
    FillRect(mem, &full, blue);
    DeleteObject(blue as HGDIOBJ);
    SetBkMode(mem, TRANSPARENT as i32);
    SetTextColor(mem, 0x00ff_ffff);
    let text = wide("E");
    let mut trect = full;
    DrawTextW(
        mem,
        text.as_ptr(),
        -1,
        &mut trect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(mem, old);

    // Mask layer: opaque everywhere except the four corners (rounded look).
    let mask = CreateBitmap(S, S, 1, 1, std::ptr::null());
    let mask_dc = CreateCompatibleDC(screen);
    let old_mask = SelectObject(mask_dc, mask as HGDIOBJ);
    let white = CreateSolidBrush(0x00ff_ffff);
    FillRect(mask_dc, &full, white);
    DeleteObject(white as HGDIOBJ);
    let black = CreateSolidBrush(0x0000_0000);
    for (l, t) in [(0, 0), (S - 4, 0), (0, S - 4), (S - 4, S - 4)] {
        let r = windows_sys::Win32::Foundation::RECT {
            left: l,
            top: t,
            right: l + 4,
            bottom: t + 4,
        };
        FillRect(mask_dc, &r, black);
    }
    DeleteObject(black as HGDIOBJ);
    SelectObject(mask_dc, old_mask);
    DeleteDC(mask_dc);

    let mut info: ICONINFO = std::mem::zeroed();
    info.fIcon = 1;
    info.hbmColor = color;
    info.hbmMask = mask;
    let icon = CreateIconIndirect(&info);
    DeleteObject(mask as HGDIOBJ);
    DeleteObject(color as HGDIOBJ);
    DeleteDC(mem);
    ReleaseDC(std::ptr::null_mut(), screen);
    icon
}

/// Add the tray ("notification area") icon: left-click opens the console,
/// right-click shows a tiny menu. This is the agent's only persistent UI.
fn add_tray_icon(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ID;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = make_tray_icon();
        let tip = wide("EduGuard Agent");
        let n = tip.len().min(nid.szTip.len());
        nid.szTip[..n].copy_from_slice(&tip[..n]);
        if Shell_NotifyIconW(NIM_ADD, &nid) == 0 {
            tracing::error!("tray: Shell_NotifyIconW(NIM_ADD) failed");
        }
        TRAY_ICON = nid.hIcon; // shell keeps a reference; destroy on exit
    }
}

fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ID;
        Shell_NotifyIconW(NIM_DELETE, &nid);
        if !TRAY_ICON.is_null() {
            DestroyIcon(TRAY_ICON);
            TRAY_ICON = std::ptr::null_mut();
        }
    }
}

unsafe fn tray_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    AppendMenuW(menu, MF_STRING, CMD_OPEN_CONSOLE, wide("Open Console").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide("EduGuard Agent").as_ptr());
    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        0,
        hwnd,
        std::ptr::null(),
    );
    DestroyMenu(menu);
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            // Legacy callback layout: wParam = icon id, lParam = mouse message.
            match (l as u32) & 0xFFFF {
                WM_LBUTTONUP => open_console(),
                WM_RBUTTONUP => tray_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            if ((w as u32) & 0xFFFF) as usize == CMD_OPEN_CONSOLE {
                open_console();
            }
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_null() {
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, 0x000000);
                let mut rect: windows_sys::Win32::Foundation::RECT = std::mem::zeroed();
                windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
                let text: Vec<u16> = if HUD_HWND == hwnd {
                    HUD_TEXT.with(|t| wide(&t.borrow()))
                } else {
                    Vec::new()
                };
                if !text.is_empty() {
                    DrawTextW(
                        hdc,
                        text.as_ptr(),
                        text.len() as i32,
                        &mut rect,
                        DT_CENTER | DT_SINGLELINE,
                    );
                }
                EndPaint(hwnd, &ps);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

thread_local! {
    static HUD_TEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn register_class(hinstance: HINSTANCE) -> u16 {
    unsafe {
        let brush = CreateSolidBrush(COLOR_WINDOW as u32);
        // NOTE: the Vec must outlive the RegisterClassW call (raw pointer).
        let class_name = wide("EduGuardOverlay");
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: brush as HBRUSH,
            lpszClassName: class_name.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&class)
    }
}

fn create_window(class: u16, hinstance: HINSTANCE, ex: WINDOW_EX_STYLE, w: i32, h: i32) -> HWND {
    unsafe {
        CreateWindowExW(
            ex,
            class as *const u16,
            wide("EduGuard Overlay").as_ptr(),
            WS_POPUP | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            w,
            h,
            0 as _,
            0 as _,
            hinstance,
            std::ptr::null(),
        )
    }
}

pub fn run(rx: Receiver<Update>) {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = register_class(hinstance);
        if class == 0 {
            tracing::error!("RegisterClassW failed");
            return;
        }
        let screen_w = windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(0);
        let screen_h = windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(1);

        // Blank window: fullscreen, top-most, click-through tool window.
        BLANK_HWND = create_window(
            class,
            hinstance,
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_LAYERED,
            screen_w,
            screen_h,
        );
        // HUD window: top strip for banner/message.
        HUD_HWND = create_window(
            class,
            hinstance,
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_LAYERED,
            900,
            90,
        );
        if BLANK_HWND.is_null() || HUD_HWND.is_null() {
            tracing::error!("CreateWindowExW failed for overlay windows");
            return;
        }
        MoveWindow(HUD_HWND, (screen_w - 900) / 2, 0, 900, 90, 1);
        ShowWindow(BLANK_HWND, 0); // hidden initially
        add_tray_icon(HUD_HWND);

        let mut ui = UiState {
            school: String::new(),
            banner_on: false,
            blanked: false,
            message: None,
        };

        let mut msg: MSG = std::mem::zeroed();
        loop {
            while let Ok(update) = rx.try_recv() {
                match update {
                    Update::Blank(b) => {
                        ui.blanked = b;
                        ShowWindow(BLANK_HWND, if b { SW_SHOW as i32 } else { 0 });
                        state::BLANKED.store(b, std::sync::atomic::Ordering::Relaxed);
                    }
                    Update::Banner(text) => {
                        ui.banner_on = text.is_some();
                        if let Some(t) = text {
                            ui.school = t;
                        }
                    }
                    Update::Message { text, ttl_secs } => {
                        ui.message =
                            Some((text, Instant::now() + Duration::from_secs(ttl_secs)));
                    }
                    Update::ClearMessage => ui.message = None,
                    Update::Quit => {
                        remove_tray_icon(HUD_HWND);
                        return;
                    }
                }
            }
            if let Some((_, until)) = &ui.message {
                if Instant::now() >= *until {
                    ui.message = None;
                }
            }
            let show_banner = ui.banner_on;
            let show_msg = ui.message.is_some();
            let line = if show_banner || show_msg {
                let mut s = String::new();
                if show_banner {
                    s.push_str(&format!(
                        "This device is monitored by {}",
                        if ui.school.is_empty() { "EduGuard" } else { &ui.school }
                    ));
                }
                if show_msg {
                    if !s.is_empty() {
                        s.push_str("  •  ");
                    }
                    s.push_str(&ui.message.as_ref().unwrap().0);
                }
                s
            } else {
                String::new()
            };
            HUD_TEXT.with(|t| *t.borrow_mut() = line.clone());
            ShowWindow(HUD_HWND, if line.is_empty() { 0 } else { SW_SHOW as i32 });
            if !line.is_empty() {
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(
                    HUD_HWND,
                    std::ptr::null(),
                    1,
                );
            }

            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    remove_tray_icon(HUD_HWND);
                    return;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }
}

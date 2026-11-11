//! Window controls for the custom title bar, done directly with Win32 on
//! Windows.
//!
//! Maximise / restore / minimise are *posted* as `WM_SYSCOMMAND`, exactly
//! like a native caption button: running them synchronously inside gpui's
//! click handler left the window unable to receive the next click (the
//! restore button looked dead right after maximising). Posting also keeps
//! the native maximise/minimise animations.

use gpui::Window;

#[cfg(windows)]
mod win32 {
    #[link(name = "user32")]
    extern "system" {
        pub fn IsZoomed(hwnd: isize) -> i32;
        pub fn ReleaseCapture() -> i32;
        pub fn SendMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
        pub fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
        pub fn GetCursorPos(p: *mut Point) -> i32;
        pub fn GetWindowRect(hwnd: isize, r: *mut Rect) -> i32;
        pub fn GetWindowPlacement(hwnd: isize, p: *mut Placement) -> i32;
        pub fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        pub fn SetCapture(hwnd: isize) -> isize;
        pub fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Point {
        pub x: i32,
        pub y: i32,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Placement {
        pub length: u32,
        pub flags: u32,
        pub show_cmd: u32,
        pub min: Point,
        pub max: Point,
        pub normal: Rect,
    }
    pub const SWP_NOSIZE: u32 = 0x0001;
    pub const SWP_NOZORDER: u32 = 0x0004;
    pub const SWP_NOACTIVATE: u32 = 0x0010;
    pub const SW_RESTORE: i32 = 9;
    pub const WM_SYSCOMMAND: u32 = 0x0112;
    pub const SC_MINIMIZE: usize = 0xF020;
    pub const SC_MAXIMIZE: usize = 0xF030;
    pub const SC_RESTORE: usize = 0xF120;
    pub const WM_NCLBUTTONDOWN: u32 = 0x00A1;
    pub const WM_CLOSE: u32 = 0x0010;
    pub const HTCAPTION: usize = 2;
    #[link(name = "dwmapi")]
    extern "system" {
        pub fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const u32, size: u32) -> i32;
    }
    /// DWMWA_BORDER_COLOR (Windows 11): colour of the 1 px window border.
    pub const DWMWA_BORDER_COLOR: u32 = 34;
}

/// Paint the window's 1 px border in Abyss's own neutral colour (0xRRGGBB),
/// so the Windows setting "show accent colour on title bars and window
/// borders" does not draw the system accent around Abyss. Only calls DWM when
/// the colour changes. No effect before Windows 11.
pub fn set_border_color(window: &Window, rgb: u32) {
    #[cfg(windows)]
    {
        use std::cell::Cell;
        thread_local!(static LAST: Cell<Option<u32>> = const { Cell::new(None) });
        if LAST.with(Cell::get) == Some(rgb) {
            return;
        }
        if let Some(h) = hwnd(window) {
            // COLORREF is 0x00BBGGRR.
            let colorref = ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff);
            unsafe { win32::DwmSetWindowAttribute(h, win32::DWMWA_BORDER_COLOR, &colorref, 4) };
            LAST.with(|l| l.set(Some(rgb)));
        }
    }
    #[cfg(not(windows))]
    let _ = (window, rgb);
}

#[cfg(windows)]
fn hwnd(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// Refresh rate (Hz) of the monitor the window is on (re-read every 2 s, so
/// moving the window to another monitor is picked up); 60 when unknown.
pub fn refresh_hz(window: &Window) -> u32 {
    use std::cell::Cell;
    thread_local!(static CACHE: Cell<Option<(std::time::Instant, u32)>> = const { Cell::new(None) });
    if let Some((at, hz)) = CACHE.with(Cell::get) {
        if at.elapsed().as_secs() < 2 {
            return hz;
        }
    }
    let hz = monitor_hz(window).filter(|hz| (24..=500).contains(hz)).unwrap_or(60);
    CACHE.with(|c| c.set(Some((std::time::Instant::now(), hz))));
    hz
}

#[cfg(windows)]
fn monitor_hz(window: &Window) -> Option<u32> {
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, DEVMODEW, ENUM_CURRENT_SETTINGS, MONITORINFO, MONITORINFOEXW,
        MONITOR_DEFAULTTONEAREST,
    };
    let h = hwnd(window)?;
    // SAFETY: plain Win32 queries on our own window; every struct is
    // zero-initialised with its size field set as the API requires.
    unsafe {
        let mon = MonitorFromWindow(h as _, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(mon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) == 0 {
            return None;
        }
        let mut mode: DEVMODEW = std::mem::zeroed();
        mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        if EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) == 0 {
            return None;
        }
        Some(mode.dmDisplayFrequency)
    }
}

#[cfg(not(windows))]
fn monitor_hz(_window: &Window) -> Option<u32> {
    None
}

/// The window's client area as BGRA pixels: (width, height, bytes). Drawn by
/// the window itself (PrintWindow), so it works for GPU-rendered content and
/// whatever covers the window.
pub fn capture_client(window: &Window) -> Option<(u32, u32, Vec<u8>)> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct BitmapInfoHeader {
            size: u32,
            width: i32,
            height: i32,
            planes: u16,
            bit_count: u16,
            compression: u32,
            size_image: u32,
            x_ppm: i32,
            y_ppm: i32,
            clr_used: u32,
            clr_important: u32,
        }
        #[link(name = "user32")]
        extern "system" {
            fn GetDC(h: isize) -> isize;
            fn ReleaseDC(h: isize, dc: isize) -> i32;
            fn GetClientRect(h: isize, r: *mut win32::Rect) -> i32;
            fn PrintWindow(h: isize, dc: isize, flags: u32) -> i32;
        }
        #[link(name = "gdi32")]
        extern "system" {
            fn CreateCompatibleDC(dc: isize) -> isize;
            fn CreateCompatibleBitmap(dc: isize, w: i32, h: i32) -> isize;
            fn SelectObject(dc: isize, o: isize) -> isize;
            fn DeleteObject(o: isize) -> i32;
            fn DeleteDC(dc: isize) -> i32;
            fn GetDIBits(dc: isize, bm: isize, start: u32, lines: u32, bits: *mut u8, info: *mut BitmapInfoHeader, usage: u32) -> i32;
        }
        let h = hwnd(window)?;
        unsafe {
            let mut r = win32::Rect::default();
            GetClientRect(h, &mut r);
            let (w, ht) = (r.right - r.left, r.bottom - r.top);
            if w <= 0 || ht <= 0 {
                return None;
            }
            let wdc = GetDC(h);
            let mdc = CreateCompatibleDC(wdc);
            let bmp = CreateCompatibleBitmap(wdc, w, ht);
            let old = SelectObject(mdc, bmp);
            // PW_CLIENTONLY | PW_RENDERFULLCONTENT
            let ok = PrintWindow(h, mdc, 1 | 2) != 0;
            let mut info = BitmapInfoHeader {
                size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                width: w,
                height: -ht,
                planes: 1,
                bit_count: 32,
                compression: 0,
                size_image: 0,
                x_ppm: 0,
                y_ppm: 0,
                clr_used: 0,
                clr_important: 0,
            };
            let mut buf = vec![0u8; (w * ht * 4) as usize];
            let lines = GetDIBits(mdc, bmp, 0, ht as u32, buf.as_mut_ptr(), &mut info, 0);
            SelectObject(mdc, old);
            DeleteObject(bmp);
            DeleteDC(mdc);
            ReleaseDC(h, wdc);
            (ok && lines > 0).then_some((w as u32, ht as u32, buf))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

/// Keep receiving mouse moves and the button release while a slider is being
/// dragged, even outside the window.
pub fn capture_mouse(window: &Window) {
    #[cfg(windows)]
    if let Some(h) = hwnd(window) {
        unsafe { win32::SetCapture(h) };
    }
    #[cfg(not(windows))]
    let _ = window;
}

pub fn release_mouse() {
    #[cfg(windows)]
    unsafe {
        win32::ReleaseCapture()
    };
}

pub fn is_maximized(window: &Window) -> bool {
    #[cfg(windows)]
    if let Some(h) = hwnd(window) {
        return unsafe { win32::IsZoomed(h) } != 0;
    }
    window.is_maximized()
}

/// Maximise, or restore when already maximised.
pub fn toggle_maximize(window: &mut Window) {
    #[cfg(windows)]
    if let Some(h) = hwnd(window) {
        let cmd = if unsafe { win32::IsZoomed(h) } != 0 { win32::SC_RESTORE } else { win32::SC_MAXIMIZE };
        unsafe { win32::PostMessageW(h, win32::WM_SYSCOMMAND, cmd, 0) };
        return;
    }
    window.zoom_window();
}

pub fn minimize(window: &mut Window) {
    #[cfg(windows)]
    if let Some(h) = hwnd(window) {
        unsafe { win32::PostMessageW(h, win32::WM_SYSCOMMAND, win32::SC_MINIMIZE, 0) };
        return;
    }
    window.minimize_window();
}

pub fn close(window: &mut Window) {
    #[cfg(windows)]
    if let Some(h) = hwnd(window) {
        unsafe { win32::PostMessageW(h, win32::WM_CLOSE, 0, 0) };
        return;
    }
    window.remove_window();
}

/// An ongoing title-bar drag: where the window's top-left sits relative to the cursor.
#[derive(Clone, Copy, Debug)]
pub struct WinDrag {
    off_x: i32,
    off_y: i32,
}

/// Start moving the window with the cursor. Done by hand (the window follows
/// the cursor on every mouse move) instead of Windows' modal move loop, which
/// ran inside gpui's event handling and either crashed or did nothing.
/// A maximised window is restored first, keeping the cursor at the same
/// relative spot of the title bar, like Windows does.
pub fn drag_begin(window: &mut Window) -> Option<WinDrag> {
    #[cfg(windows)]
    {
        let h = hwnd(window)?;
        unsafe {
            let mut c = win32::Point::default();
            let mut r = win32::Rect::default();
            win32::GetCursorPos(&mut c);
            win32::GetWindowRect(h, &mut r);
            let mut drag = WinDrag { off_x: c.x - r.left, off_y: c.y - r.top };
            if win32::IsZoomed(h) != 0 {
                let mut p = win32::Placement { length: std::mem::size_of::<win32::Placement>() as u32, ..Default::default() };
                win32::GetWindowPlacement(h, &mut p);
                let ratio = (c.x - r.left) as f32 / (r.right - r.left).max(1) as f32;
                let w = p.normal.right - p.normal.left;
                drag.off_x = (w as f32 * ratio) as i32;
                drag.off_y = (c.y - r.top).min(20);
                win32::ShowWindow(h, win32::SW_RESTORE);
                win32::SetWindowPos(
                    h,
                    0,
                    c.x - drag.off_x,
                    c.y - drag.off_y,
                    0,
                    0,
                    win32::SWP_NOSIZE | win32::SWP_NOZORDER | win32::SWP_NOACTIVATE,
                );
            }
            // Keep getting mouse moves even if the cursor outruns the window.
            win32::SetCapture(h);
            return Some(drag);
        }
    }
    #[allow(unreachable_code)]
    {
        window.start_window_move();
        None
    }
}

pub fn drag_move(window: &mut Window, drag: WinDrag) {
    #[cfg(windows)]
    if let Some(h) = hwnd(window) {
        unsafe {
            let mut c = win32::Point::default();
            win32::GetCursorPos(&mut c);
            win32::SetWindowPos(
                h,
                0,
                c.x - drag.off_x,
                c.y - drag.off_y,
                0,
                0,
                win32::SWP_NOSIZE | win32::SWP_NOZORDER | win32::SWP_NOACTIVATE,
            );
        }
    }
}

/// Finish a drag. Dropped at the top edge of the screen, the window maximises
/// (like Windows' Aero Snap).
pub fn drag_end(window: &mut Window) {
    #[cfg(windows)]
    unsafe {
        win32::ReleaseCapture();
        let mut c = win32::Point::default();
        win32::GetCursorPos(&mut c);
        if c.y <= 2 {
            if let Some(h) = hwnd(window) {
                if win32::IsZoomed(h) == 0 {
                    win32::PostMessageW(h, win32::WM_SYSCOMMAND, win32::SC_MAXIMIZE, 0);
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

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
    }
    pub const WM_SYSCOMMAND: u32 = 0x0112;
    pub const SC_MINIMIZE: usize = 0xF020;
    pub const SC_MAXIMIZE: usize = 0xF030;
    pub const SC_RESTORE: usize = 0xF120;
    pub const WM_NCLBUTTONDOWN: u32 = 0x00A1;
    pub const WM_CLOSE: u32 = 0x0010;
    pub const HTCAPTION: usize = 2;
}

#[cfg(windows)]
fn hwnd(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
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

/// Hand the drag over to Windows, as if the native title bar was grabbed
/// (keeps Aero Snap and dragging a maximised window off the top).
pub fn start_drag(window: &mut Window) {
    #[cfg(windows)]
    if let Some(h) = hwnd(window) {
        unsafe {
            win32::ReleaseCapture();
            win32::SendMessageW(h, win32::WM_NCLBUTTONDOWN, win32::HTCAPTION, 0);
        }
        return;
    }
    window.start_window_move();
}

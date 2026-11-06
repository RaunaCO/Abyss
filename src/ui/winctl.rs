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
                win32::SetWindowPos(h, 0, c.x - drag.off_x, c.y - drag.off_y, 0, 0, win32::SWP_NOSIZE | win32::SWP_NOZORDER | win32::SWP_NOACTIVATE);
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
            win32::SetWindowPos(h, 0, c.x - drag.off_x, c.y - drag.off_y, 0, 0, win32::SWP_NOSIZE | win32::SWP_NOZORDER | win32::SWP_NOACTIVATE);
        }
    }
}

pub fn drag_end() {
    #[cfg(windows)]
    unsafe {
        win32::ReleaseCapture();
    }
}

/// Hand the drag over to Windows/// Hand the drag over to Windows, as if the native title bar was grabbed
/// (keeps Aero Snap and dragging a maximised window off the top).
pub fn start_drag(window: &mut Window) {
    #[cfg(windows)]
    if let Some(h) = hwnd(window) {
        unsafe {
            win32::ReleaseCapture();
            // Posted, not sent: the move loop must start from the message pump. Run inside
            // gpui's event handler it keeps the app borrowed while gpui's own tasks (the
            // ticker) run in that loop, which panics with "RefCell already borrowed".
            win32::PostMessageW(h, win32::WM_NCLBUTTONDOWN, win32::HTCAPTION, 0);
        }
        return;
    }
    window.start_window_move();
}

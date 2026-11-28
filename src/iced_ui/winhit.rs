//! Native window chrome on Windows: the system moves, resizes, snaps and
//! maximises the window, Iced only says where its title bar is.
//!
//! The window is undecorated (winit removes the frame in `WM_NCCALCSIZE`), so
//! Windows would treat every pixel as client area. We wrap winit's window
//! procedure and answer `WM_NCHITTEST` ourselves:
//!
//! - near the window's edges (not maximised, not full screen) the resize
//!   codes, with long L-shaped corners;
//! - over the empty parts of a *caption surface* `HTCAPTION`, which gives the
//!   native move, drag-to-restore from maximised, Aero Snap, the system
//!   double-click and the system menu on right-click;
//! - everywhere else `HTCLIENT`, so Iced gets the click.
//!
//! The view publishes where those surfaces are with two transparent widgets,
//! [`caption`] and [`hole`] (an interactive control inside a caption), which
//! record their bounds while they are drawn. [`root`] wraps the whole view:
//! each draw collects a fresh map and swaps it in when the frame is done, so
//! surfaces of a screen that is no longer shown are gone with the next frame
//! (a generation per frame, without stale entries to filter).
//!
//! This replaces winit's `drag_window` / `drag_resize_window`: winit sets a
//! `dragging` flag on every call and clears it only on `WM_EXITSIZEMOVE`, so
//! a click that never moved left it set and every later drag or resize was
//! silently ignored.
//!
//! Marks are in Iced's logical units (design px). The hit test converts the
//! pointer with the window's DPI and the interface zoom, exactly as Iced lays
//! out (`physical = logical × DPI/96 × zoom`). Marks inside a scrollable
//! would be off by its offset: use them only on surfaces that do not scroll.

use std::cell::RefCell;
use std::sync::Mutex;

use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{layout, mouse, overlay, renderer, Clipboard, Layout, Shell, Widget};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// How far (design px) a caption surface reaches past its own bounds to the
/// left, right and top: across the window's frame gaps, so the strip above
/// the panels and the gap between the rail and the panel move the window too
/// and, maximised, the caption reaches the screen's very top edge.
pub const CAPTION_REACH: f32 = 8.0;
/// Minimum resize border, in px at 96 DPI (the system's is usually 8).
const MIN_BORDER: f32 = 6.0;
/// Length of each arm of the L-shaped resize corners, in px at 96 DPI.
const CORNER: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Empty space here moves the window.
    Caption,
    /// An interactive control inside a caption: Iced gets the pointer.
    Hole,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub kind: Kind,
    pub rect: Rectangle,
}

/// What the last frame drew, in drawing order (later marks are on top).
#[derive(Clone, Debug, Default)]
pub struct Map {
    pub marks: Vec<Mark>,
    /// Something modal is open (menu, dialog, search results…): the first
    /// click anywhere must reach Iced to dismiss it, so no caption.
    pub blocked: bool,
    /// The window is full screen: no caption and no resize borders.
    pub fullscreen: bool,
}

thread_local! {
    /// The map the frame being drawn is collecting (None outside a [`root`]).
    static BUILDING: RefCell<Option<Vec<Mark>>> = const { RefCell::new(None) };
}

static PUBLISHED: Mutex<Map> = Mutex::new(Map { marks: Vec::new(), blocked: false, fullscreen: false });

fn record(kind: Kind, rect: Rectangle) {
    BUILDING.with(|b| {
        if let Some(marks) = b.borrow_mut().as_mut() {
            marks.push(Mark { kind, rect });
        }
    });
}

/// A copy of the map of the last frame drawn.
pub fn published() -> Map {
    PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// ------------------------------------------------------------------ widgets

/// `content`'s empty space moves the window (see the module docs).
pub fn caption<'a, Message: 'a, Theme: 'a, Renderer: renderer::Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Element<'a, Message, Theme, Renderer> {
    Element::new(Marked { content: content.into(), role: Role::Mark(Kind::Caption) })
}

/// `content` is a control inside a caption: it keeps its clicks.
pub fn hole<'a, Message: 'a, Theme: 'a, Renderer: renderer::Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Element<'a, Message, Theme, Renderer> {
    Element::new(Marked { content: content.into(), role: Role::Mark(Kind::Hole) })
}

/// The whole view: each draw publishes the caption surfaces drawn inside it.
pub fn root<'a, Message: 'a, Theme: 'a, Renderer: renderer::Renderer + 'a>(
    content: Element<'a, Message, Theme, Renderer>,
    blocked: bool,
    fullscreen: bool,
) -> Element<'a, Message, Theme, Renderer> {
    Element::new(Marked { content, role: Role::Root { blocked, fullscreen } })
}

#[derive(Clone, Copy)]
enum Role {
    Mark(Kind),
    Root { blocked: bool, fullscreen: bool },
}

/// Transparent: the content's own tree, size and events (wrapping something
/// never resets its state), plus a note of its bounds when drawn.
struct Marked<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    role: Role,
}

impl<Message, Theme, Renderer: renderer::Renderer> Widget<Message, Theme, Renderer> for Marked<'_, Message, Theme, Renderer> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        match self.role {
            Role::Mark(kind) => {
                record(kind, layout.bounds());
                self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
            }
            Role::Root { blocked, fullscreen } => {
                BUILDING.with(|b| *b.borrow_mut() = Some(Vec::new()));
                self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
                let marks = BUILDING.with(|b| b.borrow_mut().take()).unwrap_or_default();
                let map = Map { marks, blocked, fullscreen };
                #[cfg(test)]
                tests::LAST.with(|l| *l.borrow_mut() = map.clone());
                *PUBLISHED.lock().unwrap_or_else(|e| e.into_inner()) = map;
            }
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}

// ------------------------------------------------------------------ geometry

/// Where the pointer is, in the system's terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Client,
    Caption,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Maximized,
    Fullscreen,
}

/// The window, in physical px relative to the client area's origin.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The window rectangle (winit shifts the client 1 px down, so `top` is -1).
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    /// Physical px per design px (DPI/96 × zoom).
    pub scale: f32,
    /// Resize border thickness, physical px.
    pub border: f32,
    /// Length of each arm of a resize corner, physical px.
    pub corner: f32,
}

impl Frame {
    /// The resize border and corner for a window at `dpi`, given the system's
    /// own frame thickness there (size frame + padded border, physical px).
    pub fn metrics(dpi: u32, system_frame: i32) -> (f32, f32) {
        let k = dpi.max(1) as f32 / 96.0;
        let border = (system_frame as f32).max((MIN_BORDER * k).round());
        (border, (CORNER * k).round().max(border))
    }
}

/// The system's answer for a pointer at (`x`, `y`) physical px in client coordinates.
pub fn hit(x: f32, y: f32, frame: &Frame, mode: Mode, map: &Map) -> Hit {
    let mode = if map.fullscreen { Mode::Fullscreen } else { mode };
    if mode == Mode::Fullscreen {
        return Hit::Client;
    }
    if mode == Mode::Normal {
        if let Some(edge) = edge(x, y, frame) {
            return edge;
        }
    }
    if map.blocked {
        return Hit::Client;
    }
    // The marks are in design px.
    let s = frame.scale.max(0.01);
    let p = iced::Point::new(x / s, y / s);
    for m in map.marks.iter().rev() {
        let r = match m.kind {
            Kind::Hole => m.rect,
            Kind::Caption => Rectangle {
                x: m.rect.x - CAPTION_REACH,
                y: m.rect.y - CAPTION_REACH,
                width: m.rect.width + 2.0 * CAPTION_REACH,
                height: m.rect.height + CAPTION_REACH,
            },
        };
        // Half-open, so two neighbours never both claim a pixel.
        if p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height {
            return if m.kind == Kind::Caption { Hit::Caption } else { Hit::Client };
        }
    }
    Hit::Client
}

/// The resize zone under the pointer, if any.
fn edge(x: f32, y: f32, f: &Frame) -> Option<Hit> {
    let (b, c) = (f.border, f.corner);
    let left = x < f.left + b;
    let right = x >= f.right - b;
    let top = y < f.top + b;
    let bottom = y >= f.bottom - b;
    // The corners are L-shaped: `corner` long along each edge, `border` thick.
    let near_left = x < f.left + c;
    let near_right = x >= f.right - c;
    let near_top = y < f.top + c;
    let near_bottom = y >= f.bottom - c;
    Some(if (top && near_left) || (left && near_top) {
        Hit::TopLeft
    } else if (top && near_right) || (right && near_top) {
        Hit::TopRight
    } else if (bottom && near_left) || (left && near_bottom) {
        Hit::BottomLeft
    } else if (bottom && near_right) || (right && near_bottom) {
        Hit::BottomRight
    } else if top {
        Hit::Top
    } else if bottom {
        Hit::Bottom
    } else if left {
        Hit::Left
    } else if right {
        Hit::Right
    } else {
        return None;
    })
}

// ------------------------------------------------------------------ Win32

/// The widest and tallest surface wgpu configures by default (larger ones panic).
#[cfg_attr(not(windows), allow(dead_code))]
pub const MAX_SURFACE: i32 = 8192;

#[cfg(windows)]
pub use native::install;

/// Nothing to install off Windows (Iced's own drag areas are used there).
#[cfg(not(windows))]
pub fn install(_hwnd: isize) {}

#[cfg(windows)]
mod native {
    use std::sync::atomic::{AtomicIsize, Ordering};

    use super::{Frame, Hit, Mode, MAX_SURFACE};

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct MinMaxInfo {
        reserved: Point,
        max_size: Point,
        max_position: Point,
        min_track_size: Point,
        max_track_size: Point,
    }
    #[repr(C)]
    struct WindowPos {
        hwnd: isize,
        after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    }

    const GWLP_WNDPROC: i32 = -4;
    const WM_NCHITTEST: u32 = 0x0084;
    const WM_GETMINMAXINFO: u32 = 0x0024;
    const WM_WINDOWPOSCHANGING: u32 = 0x0046;
    const SWP_NOSIZE: u32 = 0x0001;
    const SM_CXSIZEFRAME: i32 = 32;
    const SM_CXPADDEDBORDER: i32 = 92;

    #[link(name = "user32")]
    extern "system" {
        #[cfg(target_pointer_width = "64")]
        fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
        #[cfg(target_pointer_width = "32")]
        #[link_name = "SetWindowLongW"]
        fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
        fn CallWindowProcW(prev: isize, hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn ScreenToClient(hwnd: isize, p: *mut Point) -> i32;
        fn ClientToScreen(hwnd: isize, p: *mut Point) -> i32;
        fn GetWindowRect(hwnd: isize, r: *mut Rect) -> i32;
        fn IsZoomed(hwnd: isize) -> i32;
        fn GetDpiForWindow(hwnd: isize) -> u32;
        fn GetSystemMetricsForDpi(index: i32, dpi: u32) -> i32;
    }

    /// winit's window procedure, which ours forwards to.
    static PREV: AtomicIsize = AtomicIsize::new(0);

    /// Wrap the main window's procedure (once; later calls do nothing). Call
    /// it on the window's own thread.
    pub fn install(hwnd: isize) {
        if hwnd == 0 || PREV.load(Ordering::Acquire) != 0 {
            return;
        }
        // SAFETY: `hwnd` is this process's live main window; `proc` matches
        // WNDPROC and forwards everything it does not answer to the previous one,
        // which is stored before any message can reach `proc` (same thread).
        unsafe {
            let ours = proc as unsafe extern "system" fn(isize, u32, usize, isize) -> isize as usize as isize;
            // Store first: the next message on this thread may already need it.
            let prev = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours);
            if prev == 0 {
                log::warn!("chrome: could not wrap the window procedure; the window has no native caption");
                return;
            }
            PREV.store(prev, Ordering::Release);
        }
        log::info!("chrome: native hit-testing installed");
    }

    unsafe extern "system" fn proc(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize {
        let prev = PREV.load(Ordering::Acquire);
        match msg {
            WM_NCHITTEST => {
                // SAFETY: called by the system for our live window.
                return unsafe { hit_test(hwnd, lparam) };
            }
            WM_GETMINMAXINFO => {
                // SAFETY: forwarding the system's own arguments to winit's procedure.
                let r = unsafe { CallWindowProcW(prev, hwnd, msg, wparam, lparam) };
                if lparam != 0 {
                    // SAFETY: for WM_GETMINMAXINFO `lparam` points to a MINMAXINFO.
                    let mmi = unsafe { &mut *(lparam as *mut MinMaxInfo) };
                    mmi.max_track_size.x = mmi.max_track_size.x.min(MAX_SURFACE);
                    mmi.max_track_size.y = mmi.max_track_size.y.min(MAX_SURFACE);
                }
                return r;
            }
            WM_WINDOWPOSCHANGING if lparam != 0 => {
                // SAFETY: for WM_WINDOWPOSCHANGING `lparam` points to a WINDOWPOS.
                let pos = unsafe { &mut *(lparam as *mut WindowPos) };
                if pos.flags & SWP_NOSIZE == 0 {
                    // Never larger than the surface the GPU accepts (several
                    // monitors, or a SetWindowPos from outside).
                    pos.cx = pos.cx.min(MAX_SURFACE);
                    pos.cy = pos.cy.min(MAX_SURFACE);
                }
            }
            _ => {}
        }
        // SAFETY: forwarding the system's own arguments to winit's procedure.
        unsafe { CallWindowProcW(prev, hwnd, msg, wparam, lparam) }
    }

    /// WM_NCHITTEST: the pointer is in screen coordinates in `lparam`.
    unsafe fn hit_test(hwnd: isize, lparam: isize) -> isize {
        let mut p = Point { x: (lparam & 0xffff) as u16 as i16 as i32, y: ((lparam >> 16) & 0xffff) as u16 as i16 as i32 };
        let mut origin = Point::default();
        let mut wr = Rect::default();
        // SAFETY: `hwnd` is our live window; the out-parameters outlive the calls.
        let (dpi, zoomed, system_frame) = unsafe {
            ScreenToClient(hwnd, &mut p);
            ClientToScreen(hwnd, &mut origin);
            GetWindowRect(hwnd, &mut wr);
            let dpi = match GetDpiForWindow(hwnd) {
                0 => 96,
                d => d,
            };
            let frame = GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
            (dpi, IsZoomed(hwnd) != 0, frame)
        };
        let (border, corner) = Frame::metrics(dpi, system_frame);
        let frame = Frame {
            left: (wr.left - origin.x) as f32,
            top: (wr.top - origin.y) as f32,
            right: (wr.right - origin.x) as f32,
            bottom: (wr.bottom - origin.y) as f32,
            scale: dpi as f32 / 96.0 * crate::shared::zoom::zoom(),
            border,
            corner,
        };
        let mode = if zoomed { Mode::Maximized } else { Mode::Normal };
        // Pixel centres, as Iced sees the pointer.
        let map = super::published();
        code(super::hit(p.x as f32 + 0.5, p.y as f32 + 0.5, &frame, mode, &map))
    }

    fn code(hit: Hit) -> isize {
        match hit {
            Hit::Client => 1,
            Hit::Caption => 2,
            Hit::Left => 10,
            Hit::Right => 11,
            Hit::Top => 12,
            Hit::TopLeft => 13,
            Hit::TopRight => 14,
            Hit::Bottom => 15,
            Hit::BottomLeft => 16,
            Hit::BottomRight => 17,
        }
    }
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// What the last root drawn on this thread published (tests run in parallel).
        pub(super) static LAST: RefCell<Map> = RefCell::new(Map::default());
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle { x, y, width: w, height: h }
    }

    /// A window of `w` x `h` design px at `dpi` and `zoom`, laid out like
    /// Abyss's: a rail with its logo, the top bar with back/forward, a
    /// clickable crumb, the search field, the tools and the caption buttons.
    fn setup(w: f32, h: f32, dpi: u32, zoom: f32) -> (Frame, Map) {
        let s = dpi as f32 / 96.0 * zoom;
        // winit's 1 px shift: the window starts 1 physical px above the client.
        let (border, corner) = Frame::metrics(dpi, (8.0 * dpi as f32 / 96.0) as i32);
        let frame = Frame { left: 0.0, top: -1.0, right: w * s, bottom: h * s - 1.0, scale: s, border, corner };
        let bar_x = 76.0;
        let marks = vec![
            // The rail's logo row (caption).
            Mark { kind: Kind::Caption, rect: rect(7.0, 7.0, 62.0, 52.0) },
            // The top bar (caption) and its controls.
            Mark { kind: Kind::Caption, rect: rect(bar_x + 1.0, 7.0, w - bar_x - 8.0, 52.0) },
            Mark { kind: Kind::Hole, rect: rect(bar_x + 17.0, 18.0, 62.0, 30.0) },
            Mark { kind: Kind::Hole, rect: rect(bar_x + 95.0, 25.0, 40.0, 16.0) },
            Mark { kind: Kind::Hole, rect: rect(w * 0.5 - 190.0, 16.0, 380.0, 34.0) },
            Mark { kind: Kind::Hole, rect: rect(w - 7.0 - 16.0 - 112.0 - 8.0 - 96.0, 18.0, 96.0, 30.0) },
            Mark { kind: Kind::Hole, rect: rect(w - 7.0 - 16.0 - 112.0, 17.0, 112.0, 32.0) },
        ];
        (frame, Map { marks, blocked: false, fullscreen: false })
    }

    /// The answer at (`x`, `y`) design px.
    fn at(x: f32, y: f32, frame: &Frame, mode: Mode, map: &Map) -> Hit {
        hit(x * frame.scale, y * frame.scale, frame, mode, map)
    }

    const DPIS: [u32; 3] = [96, 120, 144];
    const ZOOMS: [f32; 3] = [1.0, 1.25, 0.8];

    #[test]
    fn edges_and_long_corners_resize() {
        for dpi in DPIS {
            for zoom in ZOOMS {
                let (w, h) = (1366.0, 768.0);
                let (f, m) = setup(w, h, dpi, zoom);
                let s = f.scale;
                let px = |x: f32, y: f32| hit(x, y, &f, Mode::Normal, &m);
                let (fw, fh) = (f.right, f.bottom);
                // Edges, one physical px inside.
                assert_eq!(px(fw * 0.5, 0.5), Hit::Top, "dpi {dpi} zoom {zoom}");
                assert_eq!(px(fw * 0.5, -0.5), Hit::Top, "the 1 px above the client");
                assert_eq!(px(fw * 0.5, fh - 1.0), Hit::Bottom);
                assert_eq!(px(0.5, fh * 0.5), Hit::Left);
                assert_eq!(px(fw - 0.5, fh * 0.5), Hit::Right);
                // Corners: right at the corner, and along each arm.
                assert_eq!(px(0.5, 0.5), Hit::TopLeft);
                assert_eq!(px(f.corner - 1.0, 0.5), Hit::TopLeft);
                assert_eq!(px(0.5, f.corner - 2.0), Hit::TopLeft);
                assert_eq!(px(fw - f.corner + 1.0, 0.5), Hit::TopRight);
                assert_eq!(px(fw - 0.5, f.corner - 2.0), Hit::TopRight);
                assert_eq!(px(0.5, fh - f.corner + 1.0), Hit::BottomLeft);
                assert_eq!(px(fw - f.corner + 1.0, fh - 0.5), Hit::BottomRight);
                // Past the corner's arm along the top edge: plain top.
                assert_eq!(px(f.corner + 1.0, 0.5), Hit::Top);
                // The border is at least the system's thickness and 6 px at 96 DPI.
                assert!(f.border >= 6.0 * dpi as f32 / 96.0 - 0.5);
                assert_eq!(px(fw * 0.5, f.top + f.border - 0.5), Hit::Top);
                // Just below the top border: the caption, with no dead gap.
                assert_eq!(px(fw * 0.4, f.top + f.border + 0.5), Hit::Caption, "dpi {dpi} zoom {zoom} s {s}");
            }
        }
    }

    #[test]
    fn empty_bar_moves_the_window_and_controls_keep_their_clicks() {
        for dpi in DPIS {
            for zoom in ZOOMS {
                let w = 1366.0;
                let (f, m) = setup(w, 768.0, dpi, zoom);
                let n = Mode::Normal;
                // The empty strip between the crumbs and the search field.
                assert_eq!(at(300.0, 33.0, &f, n, &m), Hit::Caption, "dpi {dpi} zoom {zoom}");
                // Between the search field and the tools.
                assert_eq!(at(w * 0.5 + 250.0, 33.0, &f, n, &m), Hit::Caption);
                // The frame gap above the panel and the strip above the logo.
                assert_eq!(at(400.0, 3.0 + f.border / f.scale, &f, n, &m), Hit::Caption);
                assert_eq!(at(38.0, 4.0 + f.border / f.scale, &f, n, &m), Hit::Caption);
                // The gap between the rail and the panel.
                assert_eq!(at(72.5, 30.0, &f, n, &m), Hit::Caption);
                // The frame gap right of the panel (inside the resize border's reach).
                assert_eq!(at(w - 3.0, 30.0, &f, n, &m), Hit::Right);
                // Controls: back/forward, a crumb, the search, tools, caption buttons.
                assert_eq!(at(76.0 + 30.0, 33.0, &f, n, &m), Hit::Client);
                assert_eq!(at(76.0 + 110.0, 33.0, &f, n, &m), Hit::Client);
                assert_eq!(at(w * 0.5, 33.0, &f, n, &m), Hit::Client);
                assert_eq!(at(w - 7.0 - 16.0 - 112.0 - 8.0 - 50.0, 33.0, &f, n, &m), Hit::Client);
                assert_eq!(at(w - 7.0 - 16.0 - 10.0, 33.0, &f, n, &m), Hit::Client);
                // Below the bar: the page.
                assert_eq!(at(400.0, 70.0, &f, n, &m), Hit::Client);
            }
        }
    }

    #[test]
    fn maximised_has_no_borders_and_the_caption_reaches_the_screen_top() {
        for dpi in DPIS {
            let w = 2048.0;
            let (mut f, m) = setup(w, 1152.0, dpi, 1.0);
            // Maximised, winit's client is the work area; the window overhangs it.
            f.top = -8.0;
            f.left = -8.0;
            let x = Mode::Maximized;
            assert_eq!(hit(f.right * 0.4, 0.5, &f, x, &m), Hit::Caption, "top screen edge at dpi {dpi}");
            assert_eq!(hit(0.5, 0.5, &f, x, &m), Hit::Caption, "top-left screen corner");
            assert_eq!(hit(0.5, f.bottom * 0.5, &f, x, &m), Hit::Client, "no left border");
            assert_eq!(hit(f.right - 0.5, 0.5, &f, x, &m), Hit::Caption, "top-right corner");
            assert_eq!(at(w - 7.0 - 16.0 - 10.0, 33.0, &f, x, &m), Hit::Client, "close button");
        }
    }

    #[test]
    fn full_screen_and_modal_states_leave_everything_to_iced() {
        let (f, mut m) = setup(1366.0, 768.0, 120, 1.0);
        m.fullscreen = true;
        assert_eq!(at(300.0, 33.0, &f, Mode::Normal, &m), Hit::Client);
        assert_eq!(hit(0.5, 0.5, &f, Mode::Normal, &m), Hit::Client, "no resize in full screen");
        m.fullscreen = false;
        m.blocked = true;
        assert_eq!(at(300.0, 33.0, &f, Mode::Normal, &m), Hit::Client, "a menu is open: the click closes it");
        assert_eq!(hit(0.5, 0.5, &f, Mode::Normal, &m), Hit::TopLeft, "resizing still works");
    }

    #[test]
    fn later_marks_win_so_overlays_cover_the_caption() {
        let (f, mut m) = setup(1366.0, 768.0, 96, 1.0);
        // The unfolded sidebar's logo row over the top bar is a hole.
        m.marks.push(Mark { kind: Kind::Hole, rect: rect(6.0, 6.0, 240.0, 54.0) });
        assert_eq!(at(150.0, 33.0, &f, Mode::Normal, &m), Hit::Client);
        assert_eq!(at(300.0, 33.0, &f, Mode::Normal, &m), Hit::Caption);
        // An empty map (nothing drawn yet): no caption, but the borders resize.
        let empty = Map::default();
        assert_eq!(at(300.0, 33.0, &f, Mode::Normal, &empty), Hit::Client);
        assert_eq!(hit(300.0, 0.5, &f, Mode::Normal, &empty), Hit::Top);
    }

    #[test]
    fn metrics_follow_the_dpi() {
        assert_eq!(Frame::metrics(96, 8), (8.0, 16.0));
        assert_eq!(Frame::metrics(120, 10), (10.0, 20.0));
        assert_eq!(Frame::metrics(144, 12), (12.0, 24.0));
        // A thin system frame still gets 6 px at 96 DPI.
        assert_eq!(Frame::metrics(96, 2), (6.0, 16.0));
        assert_eq!(Frame::metrics(192, 0), (12.0, 32.0));
    }

    #[test]
    fn root_publishes_what_the_frame_drew() {
        use iced::advanced::widget::Tree;
        use iced::widget::{column, container, Space};
        type El = Element<'static, (), iced::Theme, ()>;
        let view = |with_bar: bool| -> El {
            let bar: El = if with_bar {
                caption(container(hole(Space::new().width(40).height(20))).padding(10).width(Length::Fill))
            } else {
                Space::new().into()
            };
            root(column![bar, Space::new().width(Length::Fill).height(Length::Fill)].into(), false, false)
        };
        let size = Size::new(400.0, 300.0);
        for (with_bar, count) in [(true, 2), (false, 0)] {
            let mut el = view(with_bar);
            let mut tree = Tree::new(el.as_widget());
            let node = el.as_widget_mut().layout(&mut tree, &(), &layout::Limits::new(size, size));
            let frame = Rectangle::with_size(size);
            el.as_widget().draw(
                &tree,
                &mut (),
                &iced::Theme::Dark,
                &Default::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &frame,
            );
            let map = LAST.with(|l| l.borrow().clone());
            assert_eq!(map.marks.len(), count, "with_bar {with_bar}");
            if with_bar {
                assert_eq!(map.marks[0].kind, Kind::Caption);
                assert_eq!(map.marks[0].rect, rect(0.0, 0.0, 400.0, 40.0));
                assert_eq!(map.marks[1], Mark { kind: Kind::Hole, rect: rect(10.0, 10.0, 40.0, 20.0) });
            }
        }
        // Marks drawn outside a root are not collected.
        record(Kind::Caption, rect(0.0, 0.0, 1.0, 1.0));
        assert!(BUILDING.with(|b| b.borrow().is_none()));
    }

    /// The real view's hit map, drawn over the window's top band for review:
    /// blue = caption (moves the window), red = resize border, untinted = Iced.
    /// `ABYSS_CONFIG_DIR` / `ABYSS_CACHE_DIR` must point to a scratch profile;
    /// PNGs go to `ABYSS_SHOT_DIR` (`hit-<theme>-<state>-<w>.png`, the top 160 px,
    /// and `bar-…` without the tint).
    #[test]
    #[ignore = "requires an isolated profile and a wgpu adapter; writes PNGs for review"]
    fn chrome_hit_map_shots() {
        use crate::iced_ui::{panel_motion, theme, App, Message, FONTS, INTER};
        use crate::shared::{Load, Page};
        use iced::advanced::renderer::Headless;
        use iced::advanced::renderer::Renderer as _;
        use iced::advanced::widget::Tree;
        use iced::{Color, Pixels, Renderer};
        use std::time::{Duration, Instant};
        assert!(std::env::var_os("ABYSS_CONFIG_DIR").is_some() && std::env::var_os("ABYSS_CACHE_DIR").is_some());
        let _theme = theme::THEME_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        {
            let mut fs = iced::advanced::graphics::text::font_system().write().expect("font system");
            for f in FONTS {
                fs.load_font(std::borrow::Cow::Borrowed(f));
            }
        }
        let mut renderer: Renderer =
            iced::futures::executor::block_on(<Renderer as Headless>::new(INTER, Pixels(16.0), None)).expect("offscreen wgpu adapter");
        let (mut app, boot) = App::boot();
        drop(boot);
        app.welcome_step = None;
        app.shuffle = false;
        let playlists: Vec<crate::tidal::Playlist> = ["Late night drive", "Focus"]
            .iter()
            .enumerate()
            .map(|(i, t)| {
                crate::tidal::Playlist::parse(&serde_json::json!({"uuid": format!("pl-{i}"), "title": t, "type": "USER"})).unwrap()
            })
            .collect();
        app.my_playlists = Load::Ready(playlists.clone());
        app.playlists.insert(playlists[0].uuid.clone(), Load::Ready((playlists[0].clone(), Vec::new())));
        let dir =
            std::env::var_os("ABYSS_SHOT_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("abyss-shots"));
        let _ = std::fs::create_dir_all(&dir);
        let only = std::env::var("ABYSS_SHOTS").ok();
        type Prep = fn(&mut App);
        let states: [(&str, Prep); 5] = [
            ("home", |app| {
                app.signed_in = true;
                app.sidebar_reveal = panel_motion::Reveal::closed(panel_motion::CONTROL);
                drop(app.update(Message::Navigate(Page::Home)));
            }),
            ("playlist", |app| {
                app.signed_in = true;
                drop(app.update(Message::Navigate(Page::Playlist("pl-0".into()))));
            }),
            ("revealed", |app| {
                app.signed_in = true;
                app.sidebar_reveal.set(true);
            }),
            ("signedout", |app| {
                app.signed_in = false;
                app.sidebar_reveal = panel_motion::Reveal::closed(panel_motion::CONTROL);
                drop(app.update(Message::Navigate(Page::Settings)));
            }),
            ("welcome", |app| {
                app.signed_in = false;
                app.welcome_step = Some(0);
            }),
        ];
        for (w, h) in [(960.0, 600.0), (1366.0, 768.0), (1536.0, 864.0), (2048.0, 1152.0)] {
            let size = Size::new(w, h);
            for light in [false, true] {
                for (state, prep) in states {
                    let name = format!("{}-{state}-{w}", if light { "light" } else { "dark" });
                    if only.as_deref().is_some_and(|o| !o.split(',').any(|x| name.contains(x))) {
                        continue;
                    }
                    theme::set_light(light);
                    app.welcome_step = None;
                    prep(&mut app);
                    app.viewport = size;
                    app.page_at = Instant::now() - Duration::from_secs(2);
                    let mut tree: Option<Tree> = None;
                    let mut pixels = Vec::new();
                    for _ in 0..12 {
                        let mut view = app.view();
                        let tree = match &mut tree {
                            Some(t) => {
                                t.diff(view.as_widget());
                                t
                            }
                            None => tree.insert(Tree::new(view.as_widget())),
                        };
                        let node = view.as_widget_mut().layout(tree, &renderer, &layout::Limits::new(size, size));
                        let frame = Rectangle::with_size(size);
                        let mut messages = Vec::new();
                        view.as_widget_mut().update(
                            tree,
                            &Event::Window(iced::window::Event::RedrawRequested(Instant::now())),
                            Layout::new(&node),
                            mouse::Cursor::Unavailable,
                            &renderer,
                            &mut iced::advanced::clipboard::Null,
                            &mut Shell::new(&mut messages),
                            &frame,
                        );
                        renderer.reset(frame);
                        let theme = app.iced_theme();
                        view.as_widget().draw(
                            tree,
                            &mut renderer,
                            &theme,
                            &Default::default(),
                            Layout::new(&node),
                            mouse::Cursor::Unavailable,
                            &frame,
                        );
                        pixels = Headless::screenshot(&mut renderer, Size::new(w as u32, h as u32), 1.0, Color::BLACK);
                        std::thread::sleep(Duration::from_millis(30));
                    }
                    let map = LAST.with(|l| l.borrow().clone());
                    let (border, corner) = Frame::metrics(96, 8);
                    let f = Frame { left: 0.0, top: -1.0, right: w, bottom: h - 1.0, scale: 1.0, border, corner };
                    let band = 160usize.min(h as usize);
                    let row = w as usize * 4;
                    let plain = pixels[..band * row].to_vec();
                    for y in 0..band {
                        for x in 0..w as usize {
                            let tint: Option<[f32; 3]> = match hit(x as f32 + 0.5, y as f32 + 0.5, &f, Mode::Normal, &map) {
                                Hit::Client => None,
                                Hit::Caption => Some([40.0, 120.0, 255.0]),
                                _ => Some([255.0, 60.0, 40.0]),
                            };
                            if let Some(t) = tint {
                                let i = y * row + x * 4;
                                for c in 0..3 {
                                    pixels[i + c] = (pixels[i + c] as f32 * 0.55 + t[c] * 0.45) as u8;
                                }
                            }
                        }
                    }
                    let save = |prefix: &str, px: &[u8]| {
                        let path = dir.join(format!("{prefix}-{name}.png"));
                        image::save_buffer(&path, px, w as u32, band as u32, image::ColorType::Rgba8).expect("write the PNG");
                        println!("{}", path.display());
                    };
                    save("hit", &pixels[..band * row]);
                    save("bar", &plain);
                    println!("{name}: {} marks, blocked {}, fullscreen {}", map.marks.len(), map.blocked, map.fullscreen);
                    for m in &map.marks {
                        println!("  {:?} {:.1},{:.1} {:.1}x{:.1}", m.kind, m.rect.x, m.rect.y, m.rect.width, m.rect.height);
                    }
                }
            }
        }
        theme::set_light(false);
    }
}

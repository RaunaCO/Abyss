//! The window's own chrome: the caption buttons, the drag area of the top bar,
//! the resize edges of the undecorated window and the side panel's handle.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iced::widget::{button, column, container, mouse_area, row, Space};
use iced::window::{self, Direction};
use iced::{mouse, Alignment, Border, Element, Length, Point, Task};

use super::icons::{icon, Icon};
use super::{theme, App, Message};

/// Width of the strip that resizes the window.
const EDGE: f32 = 5.0;
/// A second press this soon after the first maximises or restores.
const DOUBLE_PRESS: Duration = Duration::from_millis(400);

#[cfg(windows)]
mod win32 {
    #[repr(C)]
    #[derive(Default)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }
    pub const SWP_NOZORDER_NOACTIVATE: u32 = 0x0004 | 0x0010;
    pub const DWMWA_BORDER_COLOR: u32 = 34;
    #[link(name = "user32")]
    extern "system" {
        pub fn GetWindowRect(hwnd: isize, r: *mut Rect) -> i32;
        pub fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    }
    #[link(name = "dwmapi")]
    extern "system" {
        pub fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const u32, size: u32) -> i32;
    }
}

/// winit keeps a 1 px line at the top of an undecorated window with a shadow,
/// also in full screen: push that line off the monitor.
pub fn cover_screen(hwnd: isize) {
    #[cfg(windows)]
    // SAFETY: `hwnd` is this process's live main window and the rect outlives the calls.
    unsafe {
        let mut r = win32::Rect::default();
        if win32::GetWindowRect(hwnd, &mut r) != 0 {
            win32::SetWindowPos(hwnd, 0, r.left, r.top - 1, r.right - r.left, r.bottom - r.top + 1, win32::SWP_NOZORDER_NOACTIVATE);
        }
    }
    #[cfg(not(windows))]
    let _ = hwnd;
}

/// Paint the DWM border in the frame's colour instead of the system accent
/// (Windows 11; earlier versions ignore it).
pub fn set_border_color(hwnd: isize, rgb: u32) {
    #[cfg(windows)]
    {
        // COLORREF is 0x00BBGGRR.
        let colorref = ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff);
        // SAFETY: `hwnd` is this process's live main window; the attribute takes a 4-byte COLORREF.
        unsafe { win32::DwmSetWindowAttribute(hwnd, win32::DWMWA_BORDER_COLOR, &colorref, 4) };
    }
    #[cfg(not(windows))]
    let _ = (hwnd, rgb);
}

static CURSOR: AtomicU64 = AtomicU64::new(0);
static FOLLOW: AtomicBool = AtomicBool::new(false);

/// Note where the pointer is without waking the interface. A message is only
/// needed while something follows the pointer: rebuilding the view on every
/// mouse move would cost up to a thousand rebuilds a second.
pub fn pointer_moved(at: Point) -> Option<Message> {
    CURSOR.store((u64::from(at.x.to_bits()) << 32) | u64::from(at.y.to_bits()), Ordering::Relaxed);
    FOLLOW.load(Ordering::Relaxed).then_some(Message::Chrome(ChromeMsg::SideMove(at.x)))
}

pub fn cursor() -> Point {
    let bits = CURSOR.load(Ordering::Relaxed);
    Point::new(f32::from_bits((bits >> 32) as u32), f32::from_bits(bits as u32))
}

#[derive(Debug, Clone)]
pub enum ChromeMsg {
    TitlePress,
    Edge(Direction),
    Maximized(bool),
    SideGrab(f32),
    SideMove(f32),
    PointerUp,
}

impl App {
    pub(super) fn chrome_update(&mut self, msg: ChromeMsg) -> Task<Message> {
        let Some(id) = self.window else { return Task::none() };
        match msg {
            ChromeMsg::TitlePress => {
                // The system's move loop swallows the release, so a double click is two presses.
                let at = cursor();
                let double = self.title_press.is_some_and(|(when, p)| when.elapsed() < DOUBLE_PRESS && p.distance(at) < 6.0);
                if double {
                    self.title_press = None;
                    return window::toggle_maximize(id);
                }
                self.title_press = Some((Instant::now(), at));
                return window::drag(id);
            }
            ChromeMsg::Edge(direction) => return window::drag_resize(id, direction),
            ChromeMsg::Maximized(on) => self.maximized = on,
            ChromeMsg::SideGrab(width) => {
                self.side_resize = Some((cursor().x, width));
                FOLLOW.store(true, Ordering::Relaxed);
            }
            ChromeMsg::SideMove(x) => {
                if let Some((start, width)) = self.side_resize {
                    let (min, max) = super::pages::side_limits(self);
                    self.settings.lock().side_panel_width = Some((width + start - x).clamp(min, max));
                }
            }
            ChromeMsg::PointerUp => {
                if self.side_resize.take().is_some() {
                    FOLLOW.store(false, Ordering::Relaxed);
                    self.save_settings();
                }
            }
        }
        Task::none()
    }
}

/// Minimise, maximise / restore and close.
/// GPUI: 34 px buttons 2 px apart in a 30 px group with 2 px padding and an edge.
const CAPTION_BUTTON_W: f32 = 34.0;
pub(super) const CAPTION_WIDTH: f32 = 3.0 * CAPTION_BUTTON_W + 2.0 * 2.0 + 2.0 * 2.0 + 2.0;

pub fn caption_buttons(app: &App) -> Element<'_, Message> {
    let caption = |glyph: Icon, close: bool, msg: Message| -> Element<'_, Message> {
        // GPUI `titlebar`: the fill fades in with the eased hover.
        let b = button(container(icon(glyph, 14.0, theme::text_secondary())).center_x(CAPTION_BUTTON_W).center_y(24))
            .padding(0)
            .style(move |_, _| {
                let h = super::wash::phase().hover;
                let fill = if close { theme::rgb(0xd13a2c) } else { theme::ink(0.10) };
                button::Style {
                    background: (h > 0.0).then(|| iced::Color { a: fill.a * h, ..fill }.into()),
                    text_color: theme::text(),
                    border: Border { radius: theme::concentric(9.0, 3.0).into(), ..Default::default() },
                    ..Default::default()
                }
            })
            .on_press(msg);
        super::wash::track(b).into()
    };
    let buttons = row![
        caption(Icon::WinMin, false, Message::WindowMinimize),
        caption(if app.maximized { Icon::WinRestore } else { Icon::WinMax }, false, Message::WindowMaximize),
        caption(Icon::WinClose, true, Message::WindowClose),
    ]
    .spacing(2)
    .align_y(Alignment::Center);
    // The group already sits on a pane of glass (top bar): no fill or border of its own.
    container(buttons).padding(2).into()
}

/// `content` with its empty space moving the window (double press: maximise).
pub fn drag_area<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    mouse_area(content).on_press(Message::Chrome(ChromeMsg::TitlePress)).into()
}

/// The caption of screens without a top bar (welcome, sign-in).
pub fn caption_strip<'a>(app: &'a App, screen: Element<'a, Message>) -> Element<'a, Message> {
    let bar = row![Space::new().width(Length::Fill).height(theme::TOP_H), caption_buttons(app)].align_y(Alignment::Center).padding([0, 16]);
    let bar = column![drag_area(bar), Space::new().height(Length::Fill)];
    super::widgets::layered(screen, vec![Some(bar.into())])
}

/// The strips along the window's edges that resize it.
pub fn resize_edges<'a>(app: &App) -> Option<Element<'a, Message>> {
    if app.fullscreen || app.maximized {
        return None;
    }
    let grip = |width: Length, height: Length, direction: Direction, cursor: mouse::Interaction| -> Element<'a, Message> {
        mouse_area(Space::new().width(width).height(height))
            .on_press(Message::Chrome(ChromeMsg::Edge(direction)))
            .interaction(cursor)
            .into()
    };
    let (edge, fill) = (Length::Fixed(EDGE), Length::Fill);
    use mouse::Interaction as I;
    let grid = column![
        row![
            grip(edge, edge, Direction::NorthWest, I::ResizingDiagonallyDown),
            grip(fill, edge, Direction::North, I::ResizingVertically),
            grip(edge, edge, Direction::NorthEast, I::ResizingDiagonallyUp),
        ],
        row![
            grip(edge, fill, Direction::West, I::ResizingHorizontally),
            Space::new().width(fill).height(fill),
            grip(edge, fill, Direction::East, I::ResizingHorizontally),
        ]
        .height(fill),
        row![
            grip(edge, edge, Direction::SouthWest, I::ResizingDiagonallyUp),
            grip(fill, edge, Direction::South, I::ResizingVertically),
            grip(edge, edge, Direction::SouthEast, I::ResizingDiagonallyDown),
        ],
    ];
    Some(grid.width(fill).height(fill).into())
}

/// The handle on the left edge of the side panel, `width` wide now.
pub fn side_handle<'a>(width: f32) -> Element<'a, Message> {
    let strip = super::wash::wash(Space::new().width(6).height(Length::Fill), super::theme::divider(), 0.0);
    mouse_area(strip).on_press(Message::Chrome(ChromeMsg::SideGrab(width))).interaction(mouse::Interaction::ResizingHorizontally).into()
}

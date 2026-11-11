//! Left sidebar, always a rail of icons that unfolds when the pointer comes
//! near it.
//!
//! The page keeps a fixed column of `RAIL_W`; the sidebar itself is a layer
//! over the page (drawn after it), so unfolding never moves or re-lays out
//! the content. Its contents use one layout at full width: every icon sits
//! where the rail shows it (centred in the first `RAIL_W` pixels), and the
//! panel's growing width simply reveals the labels, which fade in. Opening
//! follows the pointer at once; closing waits a moment after it leaves, so
//! crossing the edge by accident does not make it flicker.
//!
//! Zones: Abyss's identity; navigation, discovery and the playlists,
//! which scroll; the account panel pinned to the bottom.

use super::sc;
use gpui::{div, prelude::*, AnyElement, AnyView, App, Context, FontWeight, SharedString, Window};

use super::icons::Icon;
use super::widgets::cover;
use super::{interact, theme, AppView, Load, Page};

/// Width of the rail (and of the page's left column).
pub const RAIL_W: f32 = 76.0;
/// Pointer distance beyond the rail's edge that already opens it.
const REACH: f32 = 8.0;
/// Delay before closing once the pointer has left (s).
const CLOSE_DELAY: f32 = 0.15;
const BODY_RAIL: f32 = RAIL_W - 2.0 * theme::FRAME_GAP;

/// Small tooltip bubble.
pub struct Tip(pub SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(sc(10.))
            .py(sc(6.))
            .rounded(sc(7.))
            .bg(theme::liquid_glass(0.15))
            .border_1()
            .border_color(theme::popover_border())
            .text_color(theme::text())
            .text_size(sc(theme::TYPE_LABEL))
            .font_family(theme::FONT)
            .shadow_md()
            .child(self.0.clone())
    }
}

pub fn tip(label: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_, cx| cx.new(|_| Tip(label.clone())).into()
}

/// A navigation entry.
struct NavEntry {
    id: &'static str,
    icon: Icon,
    label: &'static str,
    go: fn(&mut AppView, &mut Context<AppView>),
    active: bool,
}

impl AppView {
    /// Space reserved for navigation in the main layout.
    pub fn sidebar_width(&self) -> f32 {
        if self.signed_in {
            RAIL_W
        } else {
            0.0
        }
    }

    /// Advance the overlay without invalidating the content's layout.
    pub fn update_sidebar(&mut self) {
        let t = self.sidebar_open();
        let expanded = theme::SIDEBAR_W.min((self.viewport_w * 0.40).max(RAIL_W));
        self.sb_w = RAIL_W + (expanded - RAIL_W) * t;
        self.sb_t = t;
    }

    /// Ask before signing out (the session is removed from this computer).
    pub fn confirm_sign_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_dialog(super::playlists::Dialog::SignOut, window, cx);
    }

    /// Whether the pointer is over the sidebar (or within a few pixels of its
    /// edge), from its position: robust while the sidebar changes width under
    /// a still pointer.
    fn sidebar_hover(&mut self, now_inside: bool, cx: &mut Context<Self>) {
        if now_inside == self.sb_hover {
            return;
        }
        if now_inside {
            self.sb_hover = true;
            self.sb_left_at = None;
        } else if self.sb_hover {
            self.sb_hover = false;
            self.sb_left_at = Some(std::time::Instant::now());
            // Look again once the delay has passed.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_secs_f32(CLOSE_DELAY + 0.02)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        cx.notify();
    }

    /// 0 = rail, 1 = unfolded (eased).
    fn sidebar_open(&mut self) -> f32 {
        let open = self.sb_hover || self.sb_left_at.is_some_and(|t| t.elapsed().as_secs_f32() < CLOSE_DELAY);
        if !open {
            self.sb_left_at = None;
        }
        interact::follow("sidebar-open", if open { 1.0 } else { 0.0 }, super::motion::seconds(super::motion::CONTROL))
    }
}

/// Fade of labels as the panel unfolds (they appear once there is room).
fn label_alpha(t: f32) -> f32 {
    let x = ((t - 0.2) / 0.6).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Inner horizontal margin of rows; the icon box after it is centred on the rail.
const ROW_MX: f32 = 6.0;
const ICON_BOX: f32 = BODY_RAIL - 2.0 * ROW_MX;

/// Width of a row's surface: the icon's square on the rail, the full row unfolded.
fn row_w(width: f32) -> f32 {
    width - 2.0 * (theme::FRAME_GAP + ROW_MX) - 2.0
}

fn nav_item(e: NavEntry, width: f32, lab: f32, cx: &mut Context<AppView>) -> AnyElement {
    let key = format!("nav-{}", e.id);
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let a = interact::follow(&format!("navsel-{}", e.id), if e.active { 1.0 } else { 0.0 }, super::motion::seconds(super::motion::CONTROL));
    let color = interact::mix(interact::mix(theme::text_secondary(), theme::text(), h), theme::text(), a);
    let go = e.go;
    let el = div()
        .id(e.id)
        .relative()
        .h(sc(40.))
        .mx(sc(ROW_MX))
        .w(sc(row_w(width)))
        .overflow_hidden()
        .flex()
        .flex_none()
        .items_center()
        .rounded(sc(11.))
        .cursor_pointer()
        .bg(interact::mix(theme::ink(0.06 * h + 0.04 * p), theme::nav_active(), a))
        .child(div().absolute().left(sc(3.)).top(sc(17.)).size(sc(5.)).rounded(sc(3.)).bg(theme::accent()).opacity(a))
        .child(div().w(sc(ICON_BOX)).flex_none().flex().justify_center().child(e.icon.el_scaled(18., color, 1.0 + 0.05 * h - 0.09 * p)))
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .truncate()
                .opacity(lab)
                .text_size(sc(13.5))
                .text_color(color)
                .font_weight(if e.active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                .child(e.label),
        )
        .when(lab < 0.5, |d| d.tooltip(tip(e.label)))
        .on_click(cx.listener(move |this, _, _, cx| go(this, cx)));
    interact::track(el, key, cx).into_any_element()
}

/// Section label ("Browse"...): structure, not a button. On the rail a short
/// centred line stands in for it.
fn section_label(text: &'static str, lab: f32, trailing: Option<AnyElement>) -> AnyElement {
    div()
        .relative()
        .h(sc(29.))
        .mt(sc(6.))
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .pl(sc(19.))
        .pr(sc(10.))
        .child(
            div()
                .absolute()
                .left(sc(BODY_RAIL / 2.0 - 10.0))
                .top(sc(14.))
                .w(sc(20.))
                .h(sc(2.))
                .rounded(sc(1.))
                .bg(theme::divider())
                .opacity(1.0 - lab),
        )
        .child(div().opacity(lab).text_size(sc(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme::text_dim()).child(text))
        .children(trailing.filter(|_| lab > 0.8))
        .into_any_element()
}

/// Small square icon button (header "+", account panel).
fn small_icon_button(
    id: &'static str,
    icon: Icon,
    label: &'static str,
    on: impl Fn(&mut AppView, &mut Window, &mut Context<AppView>) + 'static,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let key = format!("sib-{id}");
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let el = div()
        .id(id)
        .size(sc(32.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(sc(8.))
        .cursor_pointer()
        .bg(theme::ink(0.08 * h + 0.05 * p))
        .child(icon.el_scaled(16., interact::mix(theme::text_secondary(), theme::text(), h), 1.0 - 0.07 * p))
        .tooltip(tip(label))
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            on(this, window, cx)
        }));
    interact::track(el, key, cx).into_any_element()
}

/// Abyss's mark centred on the rail, and the name beside it.
fn identity(lab: f32, cx: &mut Context<AppView>) -> AnyElement {
    let key = "sb-logo".to_string();
    let (h, p) = (interact::hover(&key), interact::press(&key));
    let mark = div()
        .id("sb-logo")
        .cursor_pointer()
        .opacity(1.0 - 0.1 * p)
        .shadow(if h > 0.01 {
            vec![gpui::BoxShadow {
                color: gpui::Hsla::from(theme::ink(0.25 * h)),
                offset: gpui::point(sc(0.), sc(0.)),
                blur_radius: sc(0.),
                spread_radius: sc(2. * h),
            }]
        } else {
            vec![]
        })
        // The logo opens the app menu (File, Navigation, Playback...).
        .tooltip(tip(crate::i18n::t("menu")))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_app_menu(cx)))
        .size(sc(34.))
        .flex_none()
        .rounded(sc(9.))
        .overflow_hidden()
        .bg(gpui::rgb(0xffffff))
        .border_1()
        .border_color(theme::ink(0.12))
        .child(gpui::img(super::images::logo()).size_full());
    let mark = interact::track(mark, key, cx);
    // Same height as the top bar beside it: one line across the window.
    div()
        // Lined up with the top bar inside the page panel (below the frame gap).
        .h(sc(theme::TOP_H))
        .flex()
        .flex_none()
        .items_center()
        .child(div().w(sc(BODY_RAIL)).flex_none().flex().justify_center().child(mark))
        .child(div().opacity(lab).text_size(sc(theme::TYPE_BRAND)).font_weight(FontWeight::BOLD).child(crate::i18n::t("app.name")))
        .into_any_element()
}

/// The account, pinned to the bottom: picture (centred on the rail), name and
/// e-mail (or account ID), Settings and Sign out.
fn account_panel(view: &mut AppView, width: f32, lab: f32, cx: &mut Context<AppView>) -> AnyElement {
    let (name, email, uid, picture) = {
        let s = view.settings.lock();
        (s.username.clone(), s.user_email.clone(), s.user_id, s.user_picture.clone())
    };
    let avatar = match picture.map(|p| crate::tidal::image_url(&p, 80)) {
        Some(u) => cover(Some(u), 36., true, cx),
        None => div()
            .size(sc(36.))
            .flex_none()
            .rounded(sc(18.))
            .bg(theme::active())
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::Artist.el(17., theme::text_secondary()))
            .into_any_element(),
    };
    let line1 = name.clone().unwrap_or_else(|| crate::i18n::text("account.service", &[crate::i18n::t("tidal").to_string()]));
    // Second line: the e-mail, or the account ID when there is none or it is
    // already the name.
    let line2 = email
        .filter(|e| Some(e) != name.as_ref())
        .or_else(|| uid.map(|id| crate::i18n::text("id", &[format!("{}", id)])))
        .unwrap_or_default();
    // Keep the avatar centred on the rail at every width.
    let pad_l = BODY_RAIL / 2.0 - ROW_MX - 18.0 - 1.0;
    div()
        .flex_none()
        .mx(sc(ROW_MX))
        // A tile around the avatar on the rail, the full panel unfolded.
        .w(sc(row_w(width)))
        .overflow_hidden()
        .mb(sc(6.))
        .mt(sc(8.))
        .py(sc(7.))
        .pl(sc(pad_l))
        .pr(sc(6.))
        .relative()
        .rounded(sc(12.))
        .bg(theme::liquid_glass(0.05))
        .border_1()
        .border_color(theme::divider())
        .flex()
        .items_center()
        .gap(sc(12.))
        .child(super::glass::sheen("sidebar-account-glass", 12., 0.65))
        .child(avatar)
        .child(
            div()
                .flex_1()
                .min_w(sc(0.))
                .opacity(lab)
                .flex()
                .flex_col()
                .gap(sc(2.))
                .child(div().truncate().text_size(sc(13.)).font_weight(FontWeight::SEMIBOLD).child(line1))
                .when(!line2.is_empty(), |d| {
                    d.child(div().truncate().text_size(sc(11.5)).text_color(theme::text_secondary()).child(line2))
                }),
        )
        .when(lab > 0.8, |d| {
            d.child(
                div()
                    .opacity(lab)
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(sc(2.))
                    .child(small_icon_button(
                        "acct-settings",
                        Icon::Settings,
                        crate::i18n::t("settings"),
                        |this, _, cx| this.navigate(Page::Settings, cx),
                        cx,
                    ))
                    .child(small_icon_button(
                        "acct-signout",
                        Icon::SignOut,
                        crate::i18n::t("sign_out"),
                        |this, window, cx| this.confirm_sign_out(window, cx),
                        cx,
                    )),
            )
        })
        .into_any_element()
}

/// An inset rail whose expanded state floats over the main page.
pub fn render(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    let t = view.sb_t;
    let lab = label_alpha(t);
    let width = view.sb_w;
    let lists = view.my_playlists.ready().cloned().unwrap_or_default();
    let loading = matches!(view.my_playlists, Load::Loading);
    let lib = super::library::section(&view.page);

    let mut middle: Vec<AnyElement> = Vec::new();
    middle.push(section_label(crate::i18n::t("navigation"), lab, None));
    for e in [
        NavEntry {
            id: "nav-home",
            icon: Icon::Home,
            label: crate::i18n::t("home"),
            go: |v, cx| v.navigate(Page::Home, cx),
            active: view.page == Page::Home,
        },
        NavEntry {
            id: "nav-library",
            icon: Icon::Library,
            label: crate::i18n::t("library"),
            go: |v, cx| {
                let p = v.library_page();
                if super::library::section(&v.page).is_none() {
                    v.navigate(p, cx)
                }
            },
            active: lib.is_some(),
        },
        NavEntry {
            id: "nav-recent",
            icon: Icon::Clock,
            label: crate::i18n::t("history"),
            go: |v, cx| v.navigate(Page::Recent, cx),
            active: view.page == Page::Recent,
        },
    ] {
        middle.push(nav_item(e, width, lab, cx));
    }
    middle.push(section_label(crate::i18n::t("for_you"), lab, None));
    for e in [
        NavEntry {
            id: "nav-replay",
            icon: Icon::Stats,
            label: crate::i18n::t("replay"),
            go: |v, cx| v.navigate(Page::Replay, cx),
            active: view.page == Page::Replay,
        },
        NavEntry {
            id: "nav-discover",
            icon: Icon::Mix,
            label: crate::i18n::t("mixes_new_releases"),
            go: |v, cx| v.navigate(Page::Discover, cx),
            active: view.page == Page::Discover,
        },
    ] {
        middle.push(nav_item(e, width, lab, cx));
    }
    let new_pl = small_icon_button(
        "sidebar-new-pl",
        Icon::Plus,
        crate::i18n::t("playlist.create"),
        |this, window, cx| this.open_dialog(super::playlists::Dialog::NewPlaylist { add: vec![] }, window, cx),
        cx,
    );
    middle.push(section_label(crate::i18n::t("playlists"), lab, Some(new_pl)));
    middle.push(nav_item(
        NavEntry {
            id: "nav-all-playlists",
            icon: Icon::Playlist,
            label: crate::i18n::t("all_playlists"),
            go: |v, cx| v.navigate(Page::MyPlaylists, cx),
            active: view.page == Page::MyPlaylists,
        },
        width,
        lab,
        cx,
    ));
    if loading {
        middle.push(
            div()
                .pl(sc(BODY_RAIL))
                .py(sc(6.))
                .opacity(lab)
                .text_color(theme::text_dim())
                .text_size(sc(theme::TYPE_LABEL))
                .child(crate::i18n::t("loading"))
                .into_any_element(),
        );
    }
    for (i, p) in lists.into_iter().enumerate() {
        let active = view.page == Page::Playlist(p.uuid.clone());
        let uuid = p.uuid.clone();
        let key = format!("pl-row-{i}");
        let (h, pr) = (interact::hover(&key), interact::press(&key));
        let a = interact::follow(&format!("plsel-{i}"), if active { 1.0 } else { 0.0 }, super::motion::seconds(super::motion::CONTROL));
        let el = div()
            .id(("pl", i))
            .h(sc(46.))
            .mx(sc(ROW_MX))
            .w(sc(row_w(width)))
            .overflow_hidden()
            .flex()
            .flex_none()
            .items_center()
            .rounded(sc(10.))
            .cursor_pointer()
            .text_size(sc(13.))
            .text_color(interact::mix(interact::mix(theme::text_secondary(), theme::text(), h), theme::text(), a))
            .bg(interact::mix(theme::ink(0.06 * h + 0.04 * pr), theme::nav_active(), a))
            .child(
                div()
                    .w(sc(ICON_BOX))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(div().rounded(sc(5.)).overflow_hidden().child(view.playlist_cover(&p, 34., cx))),
            )
            .child(div().flex_1().min_w(sc(0.)).pr(sc(10.)).truncate().opacity(lab).child(SharedString::from(p.title.clone())))
            .on_click(cx.listener(move |this, _, _, cx| this.navigate(Page::Playlist(uuid.clone()), cx)));
        middle.push(interact::track(el, key, cx).into_any_element());
    }

    let panel = div()
        .id("sidebar")
        .absolute()
        .top(sc(theme::FRAME_GAP))
        .bottom(sc(theme::FRAME_GAP))
        .left(sc(theme::FRAME_GAP))
        .w(sc(width - 2.0 * theme::FRAME_GAP))
        .rounded(sc(theme::PANEL_R))
        .border_1()
        .border_color(theme::panel_border())
        .occlude()
        .overflow_hidden()
        .bg(theme::frame())
        .child(
            div()
                .w_full()
                .h_full()
                .flex()
                .flex_col()
                .child(identity(lab, cx))
                .child(
                    div()
                        .id("sidebar-scroll")
                        .flex_1()
                        .min_h(sc(0.))
                        .overflow_y_scroll()
                        .track_scroll(&super::smooth::handle("sidebar"))
                        .pb(sc(10.))
                        .flex()
                        .flex_col()
                        .gap(sc(3.))
                        .children(middle)
                        .child(super::smooth::glide("sidebar")),
                )
                .child(account_panel(view, width, lab, cx)),
        );
    // Hover from the pointer's position on every move (capture phase, so
    // menus or the page cannot hide it): over the sidebar's width plus a few
    // pixels, above the player bar.
    panel
        .child(
            gpui::canvas(|bounds, _, _| bounds, {
                let view = cx.entity();
                move |bounds: gpui::Bounds<gpui::Pixels>, _, window, _| {
                    let view = view.clone();
                    window.on_mouse_event(move |ev: &gpui::MouseMoveEvent, phase, _, cx| {
                        if phase != gpui::DispatchPhase::Capture {
                            return;
                        }
                        let x = f32::from(ev.position.x) - f32::from(bounds.origin.x);
                        let y = f32::from(ev.position.y);
                        let inside = x >= -f32::from(sc(theme::FRAME_GAP))
                            && x < f32::from(bounds.size.width) + f32::from(sc(REACH))
                            && y >= f32::from(bounds.origin.y)
                            && y < f32::from(bounds.origin.y + bounds.size.height);
                        if inside != view.read(cx).sb_hover {
                            view.update(cx, |v, cx| v.sidebar_hover(inside, cx));
                        }
                    });
                }
            })
            .absolute()
            .top(sc(0.))
            .left(sc(0.))
            .size_full(),
        )
        .into_any_element()
}

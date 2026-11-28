//! The navigation rail on the left, laid out like GPUI's (src/ui/sidebar.rs):
//! an inset panel whose icons, logo, rules and avatar share one axis. The page
//! keeps a fixed column of `RAIL_W`; hovering the rail unfolds the sidebar
//! over the page, with labels.

use iced::widget::{button, column, container, mouse_area, row, scrollable, stack, Space};
use iced::{Alignment, Border, Color, Element, Length, Padding};

use super::icons::Icon;
use super::widgets::{self, cover, line};
use super::{theme, App, Dialog, Message};
use crate::shared::Page;

pub const RAIL_W: f32 = 76.0;
/// The rail inside the inset panel and its 1 px borders.
const INNER_RAIL: f32 = RAIL_W - 2.0 * theme::FRAME_GAP - 2.0;
/// Inner horizontal margin of rows; the icon box after it is centred on the rail.
const ROW_MX: f32 = 6.0;
const ICON_BOX: f32 = INNER_RAIL - 2.0 * ROW_MX;
const ICON_LEFT: f32 = ROW_MX + (ICON_BOX - 18.0) / 2.0;
const LOGO: f32 = 30.0;

/// How far the sidebar has unfolded, read when drawing (GPUI `label_alpha`):
/// labels fade in once there is room for them while the width animates.
#[derive(Clone, Copy)]
struct Lab(Option<super::panel_motion::Reveal>);

impl Lab {
    fn get(self) -> f32 {
        self.0.map_or(0.0, |r| super::motion::label_alpha(r.fraction()))
    }
    /// Shown only once well unfolded (GPUI: `lab > 0.8`), at the labels' opacity.
    fn late(self) -> f32 {
        let l = self.get();
        if l > 0.8 {
            l
        } else {
            0.0
        }
    }
}

fn alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

/// A navigation row (GPUI `sidebar::nav_item`): the wash fades in under the
/// pointer (ink 6 %, 4 % more pressed), selection glides into the active fill
/// over `CONTROL` with its accent dot, the label brightens with the hover and
/// the icon grows 5 % (sinks 9 % pressed).
fn nav_row<'a>(
    lead: Element<'a, Message>,
    label: Option<super::ellipsis::Line<'a>>,
    height: f32,
    radius: f32,
    active: bool,
    lens: bool,
    msg: Message,
) -> Element<'a, Message> {
    let dot = container(Space::new().width(5).height(5)).style(|_| container::Style {
        background: Some(super::wash::faded(alpha(theme::accent(), super::wash::phase().active)).into()),
        border: Border { radius: 3.0.into(), ..Default::default() },
        ..Default::default()
    });
    let dot = container(dot).padding(Padding { left: 3.0, ..Default::default() }).width(0);
    let mut content = row![dot, lead].align_y(Alignment::Center);
    if let Some(l) = label {
        content = content.push(l);
    }
    let b = button(container(content).height(height).align_y(iced::alignment::Vertical::Center).clip(true))
        .width(Length::Fill)
        .padding(0)
        .style(|_, _| button::Style { text_color: theme::text(), ..Default::default() })
        .on_press(msg);
    super::wash::surface(b, move |p| super::wash::Look {
        // Under a glass lens (the main sections) the selection is the glass itself.
        background: Some(
            super::wash::mix(
                theme::ink(0.06 * p.hover + 0.04 * p.press),
                if lens { iced::Color::TRANSPARENT } else { theme::nav_active() },
                p.active,
            )
            .into(),
        ),
        border: Border { radius: radius.into(), ..Default::default() },
        ..Default::default()
    })
    .active(active)
    .into()
}

/// GPUI's row colour: secondary, brightening with the hover and the selection.
fn row_color(p: super::wash::Phase) -> Color {
    super::wash::mix(super::wash::mix(theme::text_secondary(), theme::text(), p.hover), theme::text(), p.active)
}

fn nav_item<'a>(i: Icon, label: &'a str, active: bool, open: bool, lab: Lab, msg: Message) -> Element<'a, Message> {
    let glyph =
        super::icons::glyph(i, 18.0, theme::text_secondary()).live(|p, _| row_color(p)).scale(|p| 1.0 + 0.05 * p.hover - 0.09 * p.press);
    let lead = container(glyph).width(ICON_BOX).center_x(ICON_BOX).into();
    let name = open.then(|| {
        let name = line(label, widgets::scale::TYPE_SMALL, theme::text_secondary())
            .live(move |_| alpha(row_color(super::wash::phase()), lab.get()));
        if active {
            name.font(widgets::semibold())
        } else {
            name
        }
    });
    let b = nav_row(lead, name, 40.0, 11.0, active, true, msg);
    // The selected section sits in a small lens of glass (neutral: the rail
    // behind is plain), kept in the tree when unselected so nothing re-lays out.
    let b: Element<'a, Message> = super::liquid_glass::glass(b, 11.0)
        .optics(super::liquid_glass::Optics::CONTROL)
        .on_chrome()
        .interactive()
        .presence(if active { 1.0 } else { 0.0 })
        .into();
    let el: Element<'a, Message> = if open { b } else { widgets::tip(b, label) };
    container(el).padding([0.0, ROW_MX]).into()
}

/// Small square icon button (the "+" of Playlists, the account panel), GPUI
/// `small_icon_button`: ink 8 % on hover (5 % more pressed), the glyph
/// brightening and sinking 7 % while held. `shown` is its opacity.
fn small_icon_button<'a>(i: Icon, label: &'a str, shown: Lab, msg: Message) -> Element<'a, Message> {
    let glyph = super::icons::glyph(i, 16.0, theme::text_secondary())
        .live(move |p, _| alpha(super::wash::mix(theme::text_secondary(), theme::text(), p.hover), shown.late()))
        .scale(|p| 1.0 - 0.07 * p.press);
    widgets::tip(
        super::wash::wash(button(container(glyph).center(32)).padding(0).style(widgets::still(8.0)).on_press(msg), theme::ink(0.08), 8.0)
            .press(theme::ink(0.05)),
        label,
    )
}

/// Section label; on the rail a short centred rule stands in for it, and
/// while unfolding the two cross-fade (GPUI `section_label`).
fn section<'a>(label: &'a str, open: bool, lab: Lab, trailing: Option<Element<'a, Message>>) -> Element<'a, Message> {
    let rule = container(Space::new().width(20).height(2)).style(move |_| container::Style {
        background: Some(super::wash::faded(alpha(theme::divider(), 1.0 - lab.get())).into()),
        border: Border { radius: 1.0.into(), ..Default::default() },
        ..Default::default()
    });
    let rule = container(rule)
        .height(35)
        .width(INNER_RAIL)
        .center_x(INNER_RAIL)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(Padding { bottom: 14.0, ..Default::default() });
    if !open {
        return rule.into();
    }
    let name = line(label, widgets::scale::TYPE_CAPTION, theme::text_dim()).font(widgets::semibold()).live(move |c| alpha(c, lab.get()));
    let mut r = row![container(name).width(Length::Fill)].align_y(Alignment::Center);
    if let Some(t) = trailing {
        r = r.push(t);
    }
    let labelled = container(r).width(Length::Fill).height(35).align_y(iced::alignment::Vertical::Bottom).padding(Padding {
        top: 0.0,
        bottom: 0.0,
        left: ICON_LEFT + 3.0,
        right: 10.0,
    });
    // A stack takes its size from its first layer: the label sizes it, the
    // (fading) rail rule sits under it.
    stack![labelled].push_under(rule).into()
}

/// Abyss's mark centred on the rail and, unfolded, its name (fading in).
fn identity<'a>(open: bool, lab: Lab) -> Element<'a, Message> {
    let mark = container(iced::widget::image(super::images::logo()).width(LOGO).height(LOGO)).width(INNER_RAIL).center_x(INNER_RAIL);
    let mut r = row![mark].align_y(Alignment::Center).height(theme::TOP_H);
    if open {
        r = r.push(
            container(line(crate::i18n::t("app.name"), 15.0, theme::text()).font(widgets::semibold()).live(move |c| alpha(c, lab.get())))
                .padding(Padding { left: ROW_MX + ICON_BOX - INNER_RAIL, ..Default::default() }),
        );
    }
    r.into()
}

/// The rail (`open` false), or the sidebar unfolded over the page while hovered.
pub fn view(app: &App, open: bool) -> Element<'_, Message> {
    let width = if open { theme::SIDEBAR_W.min((app.viewport.width * 0.40).max(RAIL_W)) } else { RAIL_W };
    let lab = Lab(open.then_some(app.sidebar_reveal));
    let lib = super::library::lib_section(&app.page).is_some();
    let new_pl =
        small_icon_button(Icon::Plus, crate::i18n::t("playlist.create"), lab, Message::OpenDialog(Dialog::NewPlaylist { add: Vec::new() }));
    let mut middle = column![
        section(crate::i18n::t("navigation"), open, lab, None),
        nav_item(Icon::Home, crate::i18n::t("home"), app.page == Page::Home, open, lab, Message::Navigate(Page::Home)),
        nav_item(Icon::Library, crate::i18n::t("library"), lib, open, lab, Message::Navigate(app.library_page())),
        nav_item(Icon::Clock, crate::i18n::t("history"), app.page == Page::Recent, open, lab, Message::Navigate(Page::Recent)),
        section(crate::i18n::t("for_you"), open, lab, None),
        nav_item(Icon::Stats, crate::i18n::t("replay"), app.page == Page::Replay, open, lab, Message::Navigate(Page::Replay)),
        nav_item(Icon::Mix, crate::i18n::t("mixes_new_releases"), app.page == Page::Discover, open, lab, Message::Navigate(Page::Discover)),
        section(crate::i18n::t("playlists"), open, lab, open.then_some(new_pl)),
        nav_item(
            Icon::Playlist,
            crate::i18n::t("all_playlists"),
            app.page == Page::MyPlaylists,
            open,
            lab,
            Message::Navigate(Page::MyPlaylists)
        ),
    ]
    .spacing(3);
    if let Some(list) = app.my_playlists.ready() {
        for p in list.iter().take(60) {
            let active = app.page == Page::Playlist(p.uuid.clone());
            let art = container(cover(p.cover(160), 34.0, false)).width(ICON_BOX).center_x(ICON_BOX).into();
            let name = open.then(|| {
                line(p.title.clone(), 13.0, theme::text_secondary()).live(move |_| alpha(row_color(super::wash::phase()), lab.get()))
            });
            let b = nav_row(art, name, 46.0, 10.0, active, false, Message::Navigate(Page::Playlist(p.uuid.clone())));
            let el: Element<'_, Message> = if open { b } else { widgets::tip(b, &p.title) };
            middle = middle.push(container(el).padding([0.0, ROW_MX]));
        }
    }
    let middle = super::glide::glide(
        scrollable(container(middle).padding(Padding { bottom: 10.0, ..Default::default() }))
            .height(Length::Fill)
            .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(0).scroller_width(0)))
            .style(super::pages::scroll_style),
    );

    let (name, email, uid, picture) = {
        let s = app.settings.lock();
        (s.username.clone(), s.user_email.clone(), s.user_id, s.user_picture.clone())
    };
    let avatar = cover(picture.map(|id| crate::tidal::image_url(&id, 80)), 36.0, true);
    let line1 = name.clone().unwrap_or_else(|| crate::i18n::text("account.service", &[crate::i18n::t("tidal").to_string()]));
    let line2 =
        email.filter(|e| Some(e) != name.as_ref()).or_else(|| uid.map(|id| crate::i18n::text("id", &[id.to_string()]))).unwrap_or_default();
    let pad_l = INNER_RAIL / 2.0 - ROW_MX - 18.0 - 1.0;
    let mut account = row![avatar].spacing(12).align_y(Alignment::Center);
    if open {
        let mut names = column![line(line1, 13.0, theme::text()).font(widgets::semibold()).live(move |c| alpha(c, lab.get()))].spacing(2);
        if !line2.is_empty() {
            names = names.push(line(line2, widgets::scale::TYPE_CAPTION, theme::text_secondary()).live(move |c| alpha(c, lab.get())));
        }
        account = account.push(container(names).width(Length::Fill).clip(true)).push(
            row![
                small_icon_button(Icon::Settings, crate::i18n::t("settings"), lab, Message::Navigate(Page::Settings)),
                small_icon_button(Icon::SignOut, crate::i18n::t("sign_out"), lab, Message::OpenDialog(Dialog::SignOut)),
            ]
            .spacing(2),
        );
    }
    let account = container(
        container(account).width(Length::Fill).padding(Padding { top: 7.0, bottom: 7.0, left: pad_l, right: 6.0 }).clip(true).style(|_| {
            container::Style {
                background: Some(theme::ink(0.05).into()),
                border: Border { color: theme::ink(0.10), width: 1.0, radius: 12.0.into() },
                ..Default::default()
            }
        }),
    )
    .padding(Padding { top: 8.0, bottom: 6.0, left: ROW_MX, right: ROW_MX });

    // The rail's logo row is part of the title bar (it moves the window);
    // unfolded, the sidebar over the bar keeps the pointer (see `chrome`).
    let top = container(identity(open, lab)).width(Length::Fill);
    let top = if open { super::chrome::hole(top) } else { super::chrome::caption(top) };
    let body = column![top, middle, account];
    let panel = container(body).width(Length::Fill).height(Length::Fill).clip(true).style(|_| container::Style {
        background: Some(theme::frame().into()),
        text_color: Some(theme::text()),
        border: Border { color: theme::panel_border(), width: 1.0, radius: theme::PANEL_R.into() },
        ..Default::default()
    });
    // Inset like GPUI: a gap on every side, so the rail is a panel of its own.
    if open {
        // GPUI: the width itself animates (rows, corners and border follow it)
        // while the labels fade in once there is room.
        let framed = container(panel).width(Length::Fill).height(Length::Fill).padding(theme::FRAME_GAP);
        let reveal = super::panel_motion::fluid(framed.into(), app.sidebar_reveal, RAIL_W, width);
        mouse_area(reveal).on_exit(Message::SidebarHover(false)).on_enter(Message::SidebarHover(true)).into()
    } else {
        let framed = container(panel).width(width).height(Length::Fill).padding(theme::FRAME_GAP);
        mouse_area(framed).on_enter(Message::SidebarHover(true)).into()
    }
}

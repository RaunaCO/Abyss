//! Left navigation, laid out like the official app: a narrow icon rail with no
//! panel of its own (same black as the page), two groups of icons, a "+" and
//! the playlist covers. The hamburger in the caption row expands it to show
//! labels.

use super::sc;
use gpui::{div, prelude::*, px, AnyElement, AnyView, App, Context, FontWeight, SharedString, Window};

use super::icons::Icon;
use super::widgets::cover;
use super::{theme, AppView, Load, Page};

pub const RAIL_W: f32 = 64.0;

/// Small tooltip bubble for the icon rail.
pub struct Tip(pub SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(sc(10.))
            .py(sc(6.))
            .rounded(sc(6.))
            .bg(theme::elevated())
            .border_1()
            .border_color(theme::border())
            .text_color(theme::text())
            .text_size(sc(12.))
            .font_family(theme::FONT)
            .shadow_lg()
            .child(self.0.clone())
    }
}

pub fn tip(label: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_, cx| cx.new(|_| Tip(label.clone())).into()
}

type NavEntry = (&'static str, Icon, &'static str, fn() -> Page);

const NAV_MAIN: [NavEntry; 2] = [
    ("nav-home", Icon::Home, "Inicio", || Page::Home),
    ("nav-explore", Icon::Explore, "Explorar", || Page::Explore),
];

const NAV_COLLECTION: [NavEntry; 5] = [
    ("nav-mixes", Icon::Mix, "Mixes y radios", || Page::MyMixes),
    ("nav-playlists", Icon::Playlist, "Playlists", || Page::MyPlaylists),
    ("nav-albums", Icon::Album, "Álbumes", || Page::MyAlbums),
    ("nav-tracks", Icon::Track, "Pistas", || Page::MyTracks),
    ("nav-artists", Icon::Artist, "Artistas", || Page::MyArtists),
];

impl AppView {
    pub fn sidebar_width(&self) -> f32 {
        if self.settings.lock().sidebar_collapsed {
            RAIL_W
        } else {
            theme::SIDEBAR_W
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        let from = self.sidebar_width_animated();
        self.sidebar_anim = Some((from, std::time::Instant::now()));
        {
            let mut s = self.settings.lock();
            s.sidebar_collapsed = !s.sidebar_collapsed;
            s.save();
        }
        // Card grids depend on the content width.
        self.list_len = usize::MAX - 1;
        cx.notify();
    }
}

fn nav_item(entry: NavEntry, collapsed: bool, view: &AppView, cx: &mut Context<AppView>) -> AnyElement {
    let (id, icon, label, page) = entry;
    let page = page();
    let active = view.page == page;
    let color = if active { theme::text() } else { theme::text_secondary() };
    let target = page.clone();
    let base = div()
        .id(id)
        .h(sc(32.))
        .rounded(sc(8.))
        .flex()
        .items_center()
        .cursor_pointer()
        .when(active, |d| d.bg(theme::elevated()))
        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        .active(|s| s.opacity(0.7))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.pulse(id);
            this.navigate(target.clone(), cx)
        }));
    let pop = view.pulse_scale(id);
    if collapsed {
        base.mx(sc((RAIL_W - 32.) / 2.))
            .justify_center()
            .child(icon.el(16. * pop, color))
            .tooltip(tip(label))
            .into_any_element()
    } else {
        base.mx(sc(10.))
            .px(sc(12.))
            .gap(sc(14.))
            .text_size(sc(13.))
            .text_color(color)
            .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
            .child(icon.el(16. * pop, color))
            .child(label)
            .into_any_element()
    }
}

fn divider(collapsed: bool) -> AnyElement {
    div()
        .mx(sc(if collapsed { (RAIL_W - 12.) / 2. } else { 22. }))
        .my(sc(9.))
        .h(sc(1.))
        .bg(theme::border())
        .into_any_element()
}

pub fn render(view: &mut AppView, cx: &mut Context<AppView>) -> AnyElement {
    let collapsed = view.settings.lock().sidebar_collapsed;
    let lists: Vec<crate::tidal::Playlist> = view.my_playlists.ready().cloned().unwrap_or_default();
    let loading = matches!(view.my_playlists, Load::Loading);

    let playlists: Vec<AnyElement> = lists
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let active = view.page == Page::Playlist(p.uuid.clone());
            let uuid = p.uuid.clone();
            let open = cx.listener(move |this, _, _, cx| this.navigate(Page::Playlist(uuid.clone()), cx));
            if collapsed {
                div()
                    .id(("pl", i))
                    .mx(sc((RAIL_W - 32.) / 2.))
                    .p(sc(1.))
                    .rounded(sc(7.))
                    .flex()
                    .justify_center()
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme::elevated()))
                    .hover(|s| s.bg(theme::hover()))
                    .active(|s| s.opacity(0.7))
                    .child(view.playlist_cover(&p, 30., cx))
                    .tooltip(tip(p.title.clone()))
                    .on_click(open)
                    .into_any_element()
            } else {
                div()
                    .id(("pl", i))
                    .h(sc(40.))
                    .mx(sc(10.))
                    .px(sc(8.))
                    .flex()
                    .items_center()
                    .gap(sc(10.))
                    .rounded(sc(8.))
                    .cursor_pointer()
                    .text_size(sc(12.))
                    .text_color(if active { theme::text() } else { theme::text_secondary() })
                    .when(active, |d| d.bg(theme::elevated()))
                    .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                    .child(view.playlist_cover(&p, 28., cx))
                    .child(div().flex_1().min_w(sc(0.)).truncate().child(SharedString::from(p.title.clone())))
                    .on_click(open)
                    .into_any_element()
            }
        })
        .collect();

    let new_playlist = div()
        .id("sidebar-new-pl")
        .h(sc(32.))
        .rounded(sc(8.))
        .flex()
        .items_center()
        .cursor_pointer()
        .text_color(theme::text_secondary())
        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        .active(|s| s.opacity(0.7))
        .on_click(cx.listener(|this, _, window, cx| {
            this.open_dialog(super::playlists::Dialog::NewPlaylist { add: vec![] }, window, cx)
        }));
    let new_playlist = if collapsed {
        new_playlist
            .mx(sc((RAIL_W - 32.) / 2.))
            .justify_center()
            .child(Icon::Plus.el(16., theme::text_secondary()))
            .tooltip(tip("Nueva playlist"))
    } else {
        new_playlist
            .mx(sc(10.))
            .px(sc(12.))
            .gap(sc(14.))
            .text_size(sc(13.))
            .child(Icon::Plus.el(16., theme::text_secondary()))
            .child("Nueva playlist")
    };

    let main: Vec<AnyElement> = NAV_MAIN.iter().map(|e| nav_item(*e, collapsed, view, cx)).collect();
    let collection: Vec<AnyElement> = NAV_COLLECTION.iter().map(|e| nav_item(*e, collapsed, view, cx)).collect();

    div()
        .w(sc(view.sidebar_width_animated()))
        .flex_none()
        .h_full()
        .overflow_hidden()
        .flex()
        .flex_col()
        .pt(sc(6.))
        .children(main)
        .child(divider(collapsed))
        .children(collection)
        .child(divider(collapsed))
        .child(new_playlist)
        .child(
            div()
                .id("sidebar-playlists")
                .flex_1()
                .min_h(sc(0.))
                .overflow_y_scroll()
                .py(sc(6.))
                .flex()
                .flex_col()
                .gap(sc(6.))
                .when(loading && !collapsed, |d| d.child(div().px(sc(22.)).text_color(theme::text_dim()).text_size(sc(12.)).child("Cargando…")))
                .children(playlists),
        )
        .into_any_element()
}

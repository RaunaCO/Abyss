//! Full-screen cover: clicking an album's cover in its dropdown shows it
//! centred over a dimmed window, at the best resolution TIDAL offers,
//! resampled once to the exact device-pixel size (no stretching, no blur).
//! Escape or a click outside the image closes it.

use std::time::Instant;

use gpui::{div, img, prelude::*, AnyElement, Context, MouseButton, ObjectFit, Window};

use super::{motion, sc, AppView};

const OPEN_S: f32 = 0.22;
const CLOSE_S: f32 = 0.16;

pub struct CoverView {
    url: String,
    /// A smaller copy already in the cache, shown until `url` has arrived.
    fallback: Option<String>,
    opened: Instant,
    closing: Option<Instant>,
}

impl AppView {
    pub fn open_cover_view(&mut self, url: String, fallback: Option<String>, cx: &mut Context<Self>) {
        self.cover_view = Some(CoverView { url, fallback, opened: Instant::now(), closing: None });
        cx.notify();
    }

    /// Start closing (fade out); true if a cover was open.
    pub fn close_cover_view(&mut self, cx: &mut Context<Self>) -> bool {
        match self.cover_view.as_mut() {
            Some(v) if v.closing.is_none() => {
                v.closing = Some(Instant::now());
                cx.notify();
                true
            }
            Some(_) => true,
            None => false,
        }
    }
}

pub fn overlay(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let v = view.cover_view.as_ref()?;
    let ease = |t: f32| 1.0 - (1.0 - t).powi(3);
    let t = match v.closing {
        Some(at) => {
            let k = (at.elapsed().as_secs_f32() / CLOSE_S).min(1.0);
            if k >= 1.0 {
                view.cover_view = None;
                return None;
            }
            1.0 - ease(k)
        }
        None => ease((v.opened.elapsed().as_secs_f32() / OPEN_S).min(1.0)),
    };
    if t < 1.0 || v.closing.is_some() {
        motion::want_frame("cover-view");
    }
    let url = v.url.clone();
    let fallback = v.fallback.clone();
    let vw = f32::from(window.viewport_size().width) / super::zoom();
    let vh = f32::from(window.viewport_size().height) / super::zoom();
    // Square covers: as large as fits with a margin.
    let size = (vw.min(vh) * 0.86).floor();
    // One decode at the final size; during the short zoom it is only scaled.
    let src = super::images::sized(&url, size, size, cx);
    // Until the full-resolution file is here, the cached smaller one (scaled).
    let low = if src.is_none() { fallback.and_then(|u| super::images::sized(&u, size, size, cx)) } else { None };
    let shown = size * (0.94 + 0.06 * t);
    Some(
        div()
            .id("cover-view")
            .absolute()
            .top(sc(0.))
            .left(sc(0.))
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.82 * t })
            // A click anywhere outside the image closes it.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.close_cover_view(cx);
                }),
            )
            .child(
                div()
                    .id("cover-view-image")
                    .size(sc(shown))
                    .flex_none()
                    .rounded(sc(6.))
                    .opacity(t)
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::Hsla::from(gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.5 * t }),
                        offset: gpui::point(sc(0.), sc(20.)),
                        blur_radius: sc(60.),
                        spread_radius: sc(0.),
                    }])
                    // Clicks on the image itself do not close it.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .when_some(low.or(src), |d, s| d.child(img(s).size_full().rounded(sc(6.)).object_fit(ObjectFit::Contain))),
            )
            .child(
                div()
                    .absolute()
                    .top(sc(16.))
                    .right(sc(18.))
                    .rounded(sc(16.))
                    .bg(gpui::rgba(0x252525e6))
                    .opacity(t)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(super::widgets::icon_button(
                        "cover-close",
                        super::icons::Icon::Close,
                        20.,
                        gpui::rgb(0xffffff),
                        |this, _, _, cx| {
                            this.close_cover_view(cx);
                        },
                        cx,
                    )),
            )
            .into_any_element(),
    )
}

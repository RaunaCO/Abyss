//! Settings > Equalizer: a chain of parametric filters (like Equalizer APO).
//! The chain lives in `Settings`; every change is published to the output
//! callback through `Player::set_eq`, which never blocks the audio thread.
use gpui::{canvas, div, prelude::*, relative, AnyElement, Context, Entity, FontWeight, MouseButton, MouseDownEvent};

use super::icons::Icon;
use super::input::{InputEvent, TextInput};
use super::settings::{group, row, small_button, switch};
use super::{sc, theme, AppView, Drag};
use crate::audio::dsp::{Channels, FilterKind, FilterSpec, MAX_FILTERS, MAX_GAIN_DB, MAX_PREAMP_DB, MIN_PREAMP_DB};

/// Range of the sliders (the numeric fields accept the full range).
const GAIN_SLIDER_DB: f32 = 20.0;
const PREAMP_SLIDER: (f32, f32) = (-30.0, 10.0);
const DEFAULT_RATE: u32 = 48_000;

/// The numeric fields of one filter, kept in step with `Settings::eq_filters`.
pub struct EqRow {
    freq: Entity<TextInput>,
    gain: Entity<TextInput>,
    q: Entity<TextInput>,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Freq,
    Gain,
    Q,
}

fn kind_label(kind: FilterKind) -> &'static str {
    match kind {
        FilterKind::Peaking => crate::i18n::t("eq.kind_peaking"),
        FilterKind::LowShelf => crate::i18n::t("eq.kind_low_shelf"),
        FilterKind::HighShelf => crate::i18n::t("eq.kind_high_shelf"),
        FilterKind::HighPass => crate::i18n::t("eq.kind_high_pass"),
        FilterKind::LowPass => crate::i18n::t("eq.kind_low_pass"),
    }
}

fn channel_label(channels: Channels) -> &'static str {
    match channels {
        Channels::Both => crate::i18n::t("eq.channel_both"),
        Channels::Left => crate::i18n::t("eq.channel_left"),
        Channels::Right => crate::i18n::t("eq.channel_right"),
    }
}

fn fmt_freq(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn parse(text: &str) -> Option<f32> {
    text.trim().replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite())
}

fn round_tenth(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

impl AppView {
    // ------------------------------------------------------------ state

    /// Publish the chain to the audio thread; `save` also writes `settings.json`
    /// (not while a slider is being dragged).
    fn eq_apply(&mut self, save: bool, cx: &mut Context<Self>) {
        let rate = if self.snap.output_rate > 0 { self.snap.output_rate } else { DEFAULT_RATE };
        let (cfg, preamp_changed) = {
            let mut s = self.settings.lock();
            let mut changed = false;
            if s.eq_auto_preamp {
                // Measured with the switch on, so the number is right the moment it is turned on.
                let mut measured = s.eq_config();
                measured.enabled = true;
                let pre = measured.auto_preamp(f64::from(rate));
                changed = (pre - s.eq_preamp).abs() > 0.001;
                s.eq_preamp = pre;
            }
            if save {
                s.save();
            }
            (s.eq_config(), changed)
        };
        self.player.set_eq(&cfg);
        if preamp_changed {
            let text = format!("{:.1}", cfg.preamp_db);
            if let Some(input) = &self.eq_preamp_input {
                input.update(cx, |i, cx| i.set_text(&text, cx));
            }
        }
    }

    /// Called at start-up: publish the saved chain and build the fields.
    pub fn eq_start(&mut self, cx: &mut Context<Self>) {
        self.eq_apply(false, cx);
        self.eq_sync_rows(cx);
    }

    fn eq_make_input(&mut self, cx: &mut Context<Self>) -> Entity<TextInput> {
        let input = cx.new(|cx| TextInput::new("", cx).compact());
        cx.subscribe(&input, |this, input, ev: &InputEvent, cx| {
            if let InputEvent::Submit(text) = ev {
                this.eq_field_submit(&input, text, cx);
            }
        })
        .detach();
        input
    }

    /// One row of fields per filter, with the current values in them.
    pub fn eq_sync_rows(&mut self, cx: &mut Context<Self>) {
        let filters = self.settings.lock().eq_filters.clone();
        while self.eq_rows.len() < filters.len() {
            let row = EqRow { freq: self.eq_make_input(cx), gain: self.eq_make_input(cx), q: self.eq_make_input(cx) };
            self.eq_rows.push(row);
        }
        self.eq_rows.truncate(filters.len());
        if self.eq_preamp_input.is_none() {
            self.eq_preamp_input = Some(self.eq_make_input(cx));
        }
        for (row, f) in self.eq_rows.iter().zip(&filters) {
            row.freq.update(cx, |i, cx| i.set_text(&fmt_freq(f.freq), cx));
            row.gain.update(cx, |i, cx| i.set_text(&format!("{:.1}", f.gain_db), cx));
            row.q.update(cx, |i, cx| i.set_text(&format!("{:.2}", f.q), cx));
        }
        let preamp = self.settings.lock().eq_preamp;
        if let Some(input) = &self.eq_preamp_input {
            input.update(cx, |i, cx| i.set_text(&format!("{preamp:.1}"), cx));
        }
    }

    fn eq_field_submit(&mut self, input: &Entity<TextInput>, text: &str, cx: &mut Context<Self>) {
        let id = input.entity_id();
        if self.eq_preamp_input.as_ref().is_some_and(|p| p.entity_id() == id) {
            if let Some(v) = parse(text) {
                let mut s = self.settings.lock();
                // With the automatic pre-amp on, the number is not for typing.
                if !s.eq_auto_preamp {
                    s.eq_preamp = round_tenth(v).clamp(MIN_PREAMP_DB, MAX_PREAMP_DB);
                }
            }
        } else {
            let found = self.eq_rows.iter().enumerate().find_map(|(i, r)| {
                [(r.freq.entity_id(), Field::Freq), (r.gain.entity_id(), Field::Gain), (r.q.entity_id(), Field::Q)]
                    .into_iter()
                    .find(|(e, _)| *e == id)
                    .map(|(_, f)| (i, f))
            });
            let Some((i, field)) = found else { return };
            if let Some(v) = parse(text) {
                let mut s = self.settings.lock();
                if let Some(f) = s.eq_filters.get_mut(i) {
                    match field {
                        Field::Freq => f.freq = v,
                        Field::Gain => f.gain_db = v,
                        Field::Q => f.q = v,
                    }
                    *f = f.clamped();
                }
            }
        }
        self.eq_apply(true, cx);
        // Show the value that was actually applied (clamped, or the old one if the text was not a number).
        self.eq_sync_rows(cx);
        cx.notify();
    }

    fn eq_set_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.lock().eq_enabled = on;
        self.eq_apply(true, cx);
        cx.notify();
    }

    fn eq_add(&mut self, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            if s.eq_filters.len() >= MAX_FILTERS {
                drop(s);
                self.notify_toast(crate::i18n::t("eq.full"), cx);
                return;
            }
            s.eq_filters.push(FilterSpec::new(FilterKind::Peaking));
        }
        self.eq_apply(true, cx);
        self.eq_sync_rows(cx);
        cx.notify();
    }

    fn eq_remove(&mut self, i: usize, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            if i < s.eq_filters.len() {
                s.eq_filters.remove(i);
            }
        }
        self.eq_type_open = None;
        self.eq_apply(true, cx);
        self.eq_sync_rows(cx);
        cx.notify();
    }

    fn eq_move(&mut self, i: usize, down: bool, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            let j = if down { i + 1 } else { i.wrapping_sub(1) };
            if i >= s.eq_filters.len() || j >= s.eq_filters.len() {
                return;
            }
            s.eq_filters.swap(i, j);
        }
        self.eq_type_open = None;
        self.eq_apply(true, cx);
        self.eq_sync_rows(cx);
        cx.notify();
    }

    fn eq_toggle_filter(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(f) = self.settings.lock().eq_filters.get_mut(i) {
            f.enabled = !f.enabled;
        }
        self.eq_apply(true, cx);
        cx.notify();
    }

    fn eq_set_kind(&mut self, i: usize, kind: FilterKind, cx: &mut Context<Self>) {
        if let Some(f) = self.settings.lock().eq_filters.get_mut(i) {
            // Moving between a gain filter and a pass filter swaps the usual Q (1 / Butterworth).
            if f.kind.has_gain() != kind.has_gain() && (f.q - FilterSpec::new(f.kind).q).abs() < 0.005 {
                f.q = FilterSpec::new(kind).q;
            }
            f.kind = kind;
        }
        self.eq_type_open = None;
        self.eq_apply(true, cx);
        self.eq_sync_rows(cx);
        cx.notify();
    }

    fn eq_reset(&mut self, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            s.eq_filters.clear();
            s.eq_preamp = 0.0;
        }
        self.eq_type_open = None;
        self.eq_apply(true, cx);
        self.eq_sync_rows(cx);
        cx.notify();
    }

    /// Turn the automatic pre-amp on or off. Off keeps the last value for manual tuning.
    fn eq_set_auto(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.lock().eq_auto_preamp = on;
        self.eq_apply(true, cx);
        self.eq_sync_rows(cx);
        cx.notify();
    }

    fn eq_cycle_channel(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(f) = self.settings.lock().eq_filters.get_mut(i) {
            f.channels = f.channels.next();
        }
        self.eq_apply(true, cx);
        cx.notify();
    }

    /// Read the loudest output sample since the last tick, with a slow fall so
    /// the number is readable. Returns true when the equaliser page shows a
    /// different value and needs redrawing.
    pub fn eq_poll_peak(&mut self) -> bool {
        let new = self.player.take_peak();
        let before = self.eq_peak;
        self.eq_peak = if new > self.eq_peak { new } else { self.eq_peak * 0.8 };
        if self.eq_peak < 1e-4 {
            self.eq_peak = 0.0;
        }
        let db = |v: f32| if v > 0.0 { (20.0 * v.log10() * 10.0).round() / 10.0 } else { f32::NEG_INFINITY };
        self.settings_tab == super::settings::EQ_TAB && db(before) != db(self.eq_peak)
    }

    /// A slider moved: key 0 is the pre-amp, key `i + 1` the gain of filter `i`.
    pub fn eq_drag(&mut self, key: usize, frac: f32, cx: &mut Context<Self>) {
        {
            let mut s = self.settings.lock();
            if key == 0 {
                if !s.eq_auto_preamp {
                    s.eq_preamp =
                        round_tenth(PREAMP_SLIDER.0 + frac * (PREAMP_SLIDER.1 - PREAMP_SLIDER.0)).clamp(MIN_PREAMP_DB, MAX_PREAMP_DB);
                }
            } else if let Some(f) = s.eq_filters.get_mut(key - 1) {
                if f.kind.has_gain() {
                    f.gain_db = round_tenth((frac * 2.0 - 1.0) * GAIN_SLIDER_DB).clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
                }
            }
        }
        self.eq_apply(false, cx);
        // Keep the fields in step without moving the caret of a field being typed in.
        let s = self.settings.lock().clone();
        if key == 0 {
            if let Some(input) = &self.eq_preamp_input {
                input.update(cx, |i, cx| i.set_text(&format!("{:.1}", s.eq_preamp), cx));
            }
        } else if let (Some(row), Some(f)) = (self.eq_rows.get(key - 1), s.eq_filters.get(key - 1)) {
            row.gain.update(cx, |i, cx| i.set_text(&format!("{:.1}", f.gain_db), cx));
        }
    }

    /// The drag ended: write the settings once.
    pub fn eq_commit(&mut self, cx: &mut Context<Self>) {
        self.eq_apply(true, cx);
    }

    // ------------------------------------------------------------- page

    fn eq_slider(&mut self, key: usize, frac: f32, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        div()
            .id(("eq-slider", key))
            .flex_none()
            .w(sc(140.))
            .h(sc(18.))
            .flex()
            .items_center()
            .cursor_pointer()
            .relative()
            .child(
                canvas(
                    move |b, _, cx| {
                        entity.update(cx, |v, _| {
                            v.eq_bounds.insert(key, b);
                        })
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .w_full()
                    .h(sc(4.))
                    .rounded(sc(2.))
                    .bg(theme::ink(0.18))
                    .child(div().h_full().w(relative(frac.clamp(0.0, 1.0))).rounded(sc(2.)).bg(theme::accent())),
            )
            .child(
                div()
                    .absolute()
                    .top(sc(0.))
                    .bottom(sc(0.))
                    .left(relative(frac.clamp(0.0, 1.0)))
                    .w(sc(0.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().flex_none().size(sc(12.)).rounded(sc(6.)).bg(theme::text())),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    this.drag = Some(Drag::Eq(key));
                    super::winctl::capture_mouse(window);
                    this.drag_to(ev.position, cx);
                }),
            )
            .into_any_element()
    }

    fn eq_field(&self, label: &'static str, input: &Entity<TextInput>, width: f32, enabled: bool) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(sc(6.))
            .opacity(if enabled { 1.0 } else { 0.4 })
            .child(div().text_size(sc(11.5)).text_color(theme::text_secondary()).child(label))
            .child(
                div()
                    .w(sc(width))
                    .h(sc(30.))
                    .px(sc(8.))
                    .flex()
                    .items_center()
                    .rounded(sc(8.))
                    .bg(theme::field())
                    .border_1()
                    .border_color(theme::divider())
                    .when(!enabled, |d| d.occlude())
                    .child(input.clone()),
            )
            .into_any_element()
    }

    fn eq_icon_button(
        &mut self,
        id: (&'static str, usize),
        icon: Icon,
        label: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut AppView, &mut Context<AppView>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .flex_none()
            .size(sc(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(sc(8.))
            .opacity(if enabled { 1.0 } else { 0.3 })
            .when(enabled, |d| d.cursor_pointer().hover(|x| x.bg(theme::hover())))
            .child(icon.el(14., theme::text_secondary()))
            .tooltip(super::sidebar::tip(label))
            .on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    f(this, cx)
                }
            }))
            .into_any_element()
    }

    pub(super) fn settings_equalizer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();
        let cfg = s.eq_config();
        let active = !cfg.is_bypass();
        let count = s.eq_filters.len();
        self.eq_sync_rows_if_needed(cx);

        // ------------------------------------------------ switch, status, pre-amp
        let mut status = if active { crate::i18n::t("eq.status_active").to_string() } else { crate::i18n::t("eq.status_idle").to_string() };
        if self.eq_peak > 0.0 {
            // What really reaches the device: over 0 dBFS the output is clipping.
            let db = format!("{:.1}", 20.0 * self.eq_peak.log10());
            let key = if self.eq_peak > 1.0 { "eq.clip" } else { "eq.peak" };
            status = format!("{status}  |  {}", crate::i18n::text(key, &[db]));
        }
        let preamp_frac = (s.eq_preamp - PREAMP_SLIDER.0) / (PREAMP_SLIDER.1 - PREAMP_SLIDER.0);
        let preamp_slider = self.eq_slider(0, preamp_frac, cx);
        let preamp_input = self.eq_preamp_input.clone();
        let manual = !s.eq_auto_preamp;
        let preamp_right = div()
            .flex()
            .items_center()
            .gap(sc(10.))
            .opacity(if manual { 1.0 } else { 0.5 })
            .child(preamp_slider)
            .children(preamp_input.map(|input| self.eq_field("dB", &input, 56., manual)))
            .into_any_element();
        let main = vec![
            self.toggle_row(
                "eq-enabled",
                crate::i18n::t("eq.enable"),
                crate::i18n::t("eq.enable_help"),
                s.eq_enabled,
                |this, cx| {
                    let on = !this.settings.lock().eq_enabled;
                    this.eq_set_enabled(on, cx)
                },
                cx,
            ),
            row("eq-status", status, None, None)
                .text_color(if active { theme::accent() } else { theme::text_secondary() })
                .into_any_element(),
            self.toggle_row(
                "eq-auto-preamp",
                crate::i18n::t("eq.auto_preamp"),
                crate::i18n::t("eq.auto_preamp_help"),
                s.eq_auto_preamp,
                |this, cx| {
                    let on = !this.settings.lock().eq_auto_preamp;
                    this.eq_set_auto(on, cx)
                },
                cx,
            ),
            row("eq-preamp", crate::i18n::t("eq.preamp"), Some(crate::i18n::t("eq.preamp_help").into()), Some(preamp_right))
                .into_any_element(),
        ];

        // ------------------------------------------------ the chain
        let mut chain: Vec<AnyElement> = Vec::new();
        if count == 0 {
            chain.push(row("eq-empty", crate::i18n::t("eq.empty"), None, None).text_color(theme::text_secondary()).into_any_element());
        }
        for (i, f) in s.eq_filters.iter().enumerate() {
            let Some(r) = self.eq_rows.get(i) else { continue };
            let (freq, gain, q) = (r.freq.clone(), r.gain.clone(), r.q.clone());
            let on = f.enabled;
            let kind = f.kind;
            let has_gain = kind.has_gain();
            let open = self.eq_type_open == Some(i);
            let gain_frac = (f.gain_db / GAIN_SLIDER_DB + 1.0) / 2.0;
            let slider = self.eq_slider(i + 1, gain_frac, cx);
            let toggle = div()
                .id(("eq-on", i))
                .cursor_pointer()
                .child(switch(on, &format!("eq-on-{i}")))
                .on_click(cx.listener(move |this, _, _, cx| this.eq_toggle_filter(i, cx)))
                .into_any_element();
            let kind_button = small_button(("eq-kind", i), kind_label(kind), false)
                .child(Icon::ChevDown.el(12., theme::text_secondary()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.eq_type_open = if this.eq_type_open == Some(i) { None } else { Some(i) };
                    cx.notify();
                }))
                .into_any_element();
            let channel_button = small_button(("eq-channel", i), channel_label(f.channels), false)
                .on_click(cx.listener(move |this, _, _, cx| this.eq_cycle_channel(i, cx)))
                .into_any_element();
            let up = self.eq_icon_button(("eq-up", i), Icon::ChevUp, crate::i18n::t("eq.move_up"), i > 0, cx, move |this, cx| {
                this.eq_move(i, false, cx)
            });
            let down =
                self.eq_icon_button(("eq-down", i), Icon::ChevDown, crate::i18n::t("eq.move_down"), i + 1 < count, cx, move |this, cx| {
                    this.eq_move(i, true, cx)
                });
            let remove = self.eq_icon_button(("eq-remove", i), Icon::Close, crate::i18n::t("eq.remove"), true, cx, move |this, cx| {
                this.eq_remove(i, cx)
            });
            let mut item = div()
                .id(("eq-filter", i))
                .px(sc(super::widgets::PANEL_PAD - 4.))
                .py(sc(10.))
                .flex()
                .flex_col()
                .gap(sc(10.))
                .opacity(if on { 1.0 } else { 0.55 })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(sc(10.))
                        .child(toggle)
                        .child(kind_button)
                        .child(channel_button)
                        .child(div().flex_1())
                        .child(up)
                        .child(down)
                        .child(remove),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x(sc(14.))
                        .gap_y(sc(8.))
                        .child(self.eq_field(crate::i18n::t("eq.freq"), &freq, 74., true))
                        .child(self.eq_field(crate::i18n::t("eq.gain"), &gain, 64., has_gain))
                        .child(div().when(!has_gain, |d| d.opacity(0.3)).child(slider))
                        .child(self.eq_field(crate::i18n::t("eq.q"), &q, 56., true)),
                );
            if open {
                item = item.child(div().flex().flex_col().children(FilterKind::ALL.into_iter().enumerate().map(|(k, option)| {
                    let selected = option == kind;
                    div()
                        .id(("eq-kind-option", i * 8 + k))
                        .px(sc(10.))
                        .py(sc(8.))
                        .rounded(sc(6.))
                        .cursor_pointer()
                        .text_size(sc(theme::TYPE_LABEL))
                        .when(selected, |x| x.bg(theme::hover()).font_weight(FontWeight::SEMIBOLD))
                        .hover(|x| x.bg(theme::field()))
                        .child(kind_label(option))
                        .on_click(cx.listener(move |this, _, _, cx| this.eq_set_kind(i, option, cx)))
                })));
            }
            chain.push(item.into_any_element());
        }
        let buttons = div()
            .px(sc(super::widgets::PANEL_PAD - 4.))
            .py(sc(12.))
            .flex()
            .gap(sc(10.))
            .child(
                small_button("eq-add", crate::i18n::t("eq.add"), true)
                    .child(Icon::Plus.el(12., theme::button_text()))
                    .on_click(cx.listener(|v, _, _, cx| v.eq_add(cx))),
            )
            .child(small_button("eq-reset", crate::i18n::t("eq.reset"), false).on_click(cx.listener(|v, _, _, cx| v.eq_reset(cx))))
            .into_any_element();
        chain.push(buttons);

        div()
            .flex()
            .flex_col()
            .gap(sc(super::widgets::PANEL_GAP))
            .child(group(Icon::VolumeUp, crate::i18n::t("eq.title"), crate::i18n::t("eq.description"), main))
            .child(group(Icon::Library, crate::i18n::t("eq.filters"), crate::i18n::t("eq.filters_description"), chain))
            .into_any_element()
    }

    /// The fields follow the chain; rebuild them if the chain changed without
    /// going through this page (for instance on the first visit).
    fn eq_sync_rows_if_needed(&mut self, cx: &mut Context<Self>) {
        if self.eq_rows.len() != self.settings.lock().eq_filters.len() || self.eq_preamp_input.is_none() {
            self.eq_sync_rows(cx);
        }
    }
}

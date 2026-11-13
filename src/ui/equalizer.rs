//! Settings > Equalizer: a chain of parametric filters (like Equalizer APO).
//! The chain lives in `Settings`; every change is published to the output
//! callback through `Player::set_eq`, which never blocks the audio thread.
use gpui::{canvas, div, prelude::*, relative, AnyElement, Context, Entity, FontWeight, MouseButton, MouseDownEvent};

use super::eqgraph;
use super::icons::Icon;
use super::input::{InputEvent, TextInput};
use super::settings::{group, row, small_button, switch};
use super::{sc, theme, AppView, Drag};
use crate::audio::dsp::{Channels, FilterKind, FilterSpec, MAX_FILTERS, MAX_FREQ, MAX_GAIN_DB, MAX_PREAMP_DB, MIN_FREQ, MIN_PREAMP_DB};

/// Range of the sliders (the numeric fields accept the full range).
const GAIN_SLIDER_DB: f32 = 20.0;
const PREAMP_SLIDER: (f32, f32) = (-30.0, 10.0);
const DEFAULT_RATE: u32 = 48_000;

/// Slider keys (see `eq_drag`): one block of `MAX_FILTERS` per parameter.
const KEY_PREAMP: usize = 0;
const KEY_GAIN: usize = 1;
const KEY_FREQ: usize = 100;
const KEY_Q: usize = 200;
/// The channel gain faders (left, right).
const KEY_TRIM: usize = 300;
/// Height of the response graph and of the meters beside it.
const GRAPH_HEIGHT: f32 = 240.0;
/// Margins around the plot area: dB labels on the left, frequency labels at the bottom.
const GUTTER_L: f32 = 40.0;
const GUTTER_R: f32 = 10.0;
const GUTTER_T: f32 = 18.0;
const GUTTER_B: f32 = 22.0;
/// Width of the meter panel: the scale and one column per channel.
const METER_PANEL_W: f32 = 26.0 + 2.0 * 54.0 + 8.0;
/// Widths of the columns of the filter list (the header uses the same).
const COL_ON: f32 = 42.0;
const COL_NUM: f32 = 20.0;
const COL_TYPE: f32 = 128.0;
const COL_CHANNEL: f32 = 92.0;
const COL_FREQ: f32 = 82.0;
const COL_GAIN: f32 = 70.0;
const COL_Q: f32 = 62.0;

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
            // The new filter is the one to adjust next.
            self.eq_selected = Some(s.eq_filters.len() - 1);
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
        // Keep pointing at the same filter, or at none if it was the one removed.
        self.eq_selected = match self.eq_selected {
            Some(sel) if sel == i => None,
            Some(sel) if sel > i => Some(sel - 1),
            other => other,
        };
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
        // The selection follows the filter that moved.
        let j = if down { i + 1 } else { i.wrapping_sub(1) };
        if self.eq_selected == Some(i) {
            self.eq_selected = Some(j);
        } else if self.eq_selected == Some(j) {
            self.eq_selected = Some(i);
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
        self.eq_selected = None;
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

    /// Start the meter refresh (about 30 times a second) if it is not running.
    /// It only runs while the equaliser page is open, so the idle cost of the app
    /// does not change; it stops itself when the page is left.
    fn eq_start_meter(&mut self, cx: &mut Context<Self>) {
        if self.eq_meter_running {
            return;
        }
        self.eq_meter_running = true;
        // What piled up while the page was closed is not news.
        let _ = self.player.take_peaks();
        self.eq_meter = Default::default();
        cx.spawn(async move |this, cx| {
            let step = std::time::Duration::from_millis(33);
            loop {
                cx.background_executor().timer(step).await;
                let keep = this.update(cx, |this, cx| this.eq_meter_tick(step.as_secs_f32(), cx));
                if !keep.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One refresh of the meters. Returns false when the page was left.
    fn eq_meter_tick(&mut self, dt: f32, cx: &mut Context<Self>) -> bool {
        if self.settings_tab != super::settings::EQ_TAB {
            self.eq_meter_running = false;
            return false;
        }
        let peaks = self.player.take_peaks();
        self.eq_meter.update(peaks, dt);
        cx.notify();
        true
    }

    /// Set the extra gain of a channel (0 left, 1 right) and publish it.
    fn eq_set_trim(&mut self, ch: usize, db: f32, save: bool, cx: &mut Context<Self>) {
        self.settings.lock().eq_trim[ch] = db.clamp(-super::eqmeter::TRIM_RANGE_DB, super::eqmeter::TRIM_RANGE_DB);
        self.eq_apply(save, cx);
        cx.notify();
    }

    /// A slider moved. Keys: `KEY_PREAMP`, `KEY_GAIN + i`, `KEY_FREQ + i` and `KEY_Q + i`
    /// for filter `i`.
    pub fn eq_drag(&mut self, key: usize, frac: f32, cx: &mut Context<Self>) {
        if (KEY_TRIM..KEY_TRIM + 2).contains(&key) {
            // A channel gain fader: it does not touch the pre-amp or the filters.
            self.eq_set_trim(key - KEY_TRIM, super::eqmeter::trim_of_frac(frac), false, cx);
            return;
        }
        let target = if key == KEY_PREAMP {
            None
        } else if (KEY_GAIN..KEY_GAIN + MAX_FILTERS).contains(&key) {
            Some((key - KEY_GAIN, Field::Gain))
        } else if (KEY_FREQ..KEY_FREQ + MAX_FILTERS).contains(&key) {
            Some((key - KEY_FREQ, Field::Freq))
        } else if (KEY_Q..KEY_Q + MAX_FILTERS).contains(&key) {
            Some((key - KEY_Q, Field::Q))
        } else {
            return;
        };
        {
            let mut s = self.settings.lock();
            match target {
                None => {
                    if !s.eq_auto_preamp {
                        s.eq_preamp =
                            round_tenth(PREAMP_SLIDER.0 + frac * (PREAMP_SLIDER.1 - PREAMP_SLIDER.0)).clamp(MIN_PREAMP_DB, MAX_PREAMP_DB);
                    }
                }
                Some((i, field)) => {
                    if let Some(f) = s.eq_filters.get_mut(i) {
                        match field {
                            Field::Gain if f.kind.has_gain() => {
                                f.gain_db = round_tenth((frac * 2.0 - 1.0) * GAIN_SLIDER_DB).clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
                            }
                            Field::Freq => f.freq = (eqgraph::freq_of_frac(f64::from(frac)) as f32).round().clamp(MIN_FREQ, MAX_FREQ),
                            Field::Q => f.q = (eqgraph::q_of_frac(frac) * 100.0).round() / 100.0,
                            Field::Gain => {}
                        }
                        *f = f.clamped();
                    }
                }
            }
        }
        self.eq_apply(false, cx);
        // Keep the fields in step.
        let s = self.settings.lock().clone();
        match target {
            None => {
                if let Some(input) = &self.eq_preamp_input {
                    input.update(cx, |i, cx| i.set_text(&format!("{:.1}", s.eq_preamp), cx));
                }
            }
            Some((i, field)) => {
                if let (Some(row), Some(f)) = (self.eq_rows.get(i), s.eq_filters.get(i)) {
                    match field {
                        Field::Gain => row.gain.update(cx, |t, cx| t.set_text(&format!("{:.1}", f.gain_db), cx)),
                        Field::Freq => row.freq.update(cx, |t, cx| t.set_text(&fmt_freq(f.freq), cx)),
                        Field::Q => row.q.update(cx, |t, cx| t.set_text(&format!("{:.2}", f.q), cx)),
                    }
                }
            }
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

    /// A numeric field of the filter list (the column header says what it is).
    fn eq_cell(&self, input: &Entity<TextInput>, width: f32, enabled: bool) -> AnyElement {
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
            .opacity(if enabled { 1.0 } else { 0.4 })
            .when(!enabled, |d| d.occlude())
            .child(input.clone())
            .into_any_element()
    }

    /// Column titles over the filter list.
    fn eq_header(&self) -> AnyElement {
        let title = |text: &'static str, width: f32| div().w(sc(width)).child(text);
        div()
            .px(sc(super::widgets::PANEL_PAD - 4.))
            .pt(sc(8.))
            .pb(sc(2.))
            .flex()
            .flex_wrap()
            .gap_x(sc(8.))
            .text_size(sc(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text_secondary())
            .child(title("", COL_ON))
            .child(title("#", COL_NUM))
            .child(title(crate::i18n::t("eq.col_type"), COL_TYPE))
            .child(title(crate::i18n::t("eq.col_channel"), COL_CHANNEL))
            .child(title(crate::i18n::t("eq.col_freq"), COL_FREQ))
            .child(title(crate::i18n::t("eq.col_gain"), COL_GAIN))
            .child(title(crate::i18n::t("eq.col_q"), COL_Q))
            .into_any_element()
    }

    /// Sliders for the selected filter, below the list.
    fn eq_detail(&mut self, i: usize, f: &FilterSpec, cx: &mut Context<Self>) -> AnyElement {
        let gain_frac = (f.gain_db / GAIN_SLIDER_DB + 1.0) / 2.0;
        let freq_frac = eqgraph::frac_of_freq(f64::from(f.freq));
        let q_frac = eqgraph::frac_of_q(f.q);
        let has_gain = f.kind.has_gain();
        let freq = self.eq_slider(KEY_FREQ + i, freq_frac, cx);
        let gain = self.eq_slider(KEY_GAIN + i, gain_frac, cx);
        let q = self.eq_slider(KEY_Q + i, q_frac, cx);
        let labelled = |label: &'static str, value: String, slider: AnyElement, dim: bool| {
            div()
                .flex()
                .items_center()
                .gap(sc(10.))
                .opacity(if dim { 0.4 } else { 1.0 })
                .child(div().w(sc(110.)).text_size(sc(12.)).text_color(theme::text_secondary()).child(label))
                .child(slider)
                .child(div().w(sc(64.)).text_size(sc(12.)).child(value))
        };
        div()
            .px(sc(super::widgets::PANEL_PAD - 4.))
            .py(sc(10.))
            .flex()
            .flex_col()
            .gap(sc(8.))
            .border_t_1()
            .border_color(theme::divider())
            .child(labelled(crate::i18n::t("eq.freq"), format!("{} Hz", fmt_freq(f.freq)), freq, false))
            .child(labelled(crate::i18n::t("eq.gain"), format!("{:+.1} dB", f.gain_db), gain, !has_gain))
            .child(labelled(crate::i18n::t("eq.q"), format!("{:.2}", f.q), q, false))
            .into_any_element()
    }

    /// The frequency response of the chain (see `eqgraph`).
    fn eq_graph(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();
        let cfg = s.eq_config();
        let rate = f64::from(if self.snap.output_rate > 0 { self.snap.output_rate } else { DEFAULT_RATE });
        let freqs = eqgraph::freqs();
        let left = cfg.response_curve(0, rate, &freqs);
        let right = cfg.response_curve(1, rate, &freqs);
        // Two curves only when the channels differ.
        let differ = left.iter().zip(&right).any(|(a, b)| (a - b).abs() > 0.05);
        let scale = self.eq_scale.unwrap_or_else(|| eqgraph::scale_for(&[&left, &right]));
        let (c_left, c_right) = (gpui::Hsla::from(theme::accent()), gpui::Hsla::from(theme::gold()));
        let mut curves = vec![eqgraph::Curve { db: left, color: c_left }];
        if differ {
            curves.push(eqgraph::Curve { db: right, color: c_right });
        }
        let (grid, zero) = (gpui::Hsla::from(theme::ink(0.07)), gpui::Hsla::from(theme::ink(0.22)));
        let label = || div().text_size(sc(10.)).text_color(theme::text_dim());
        // The curves and the grid are painted only inside the plot area; the labels have
        // margins of their own, so nothing sits on a border or a corner.
        let plot = div().absolute().left(sc(GUTTER_L)).right(sc(GUTTER_R)).top(sc(GUTTER_T)).bottom(sc(GUTTER_B)).child(
            canvas(|bounds, _, _| bounds, move |bounds, _, window, _| eqgraph::paint(bounds, &curves, scale, grid, zero, window))
                .absolute()
                .size_full(),
        );
        // Levels down the left margin, level with their grid lines.
        let mut db_labels = div().absolute().left(sc(0.)).w(sc(GUTTER_L)).top(sc(GUTTER_T)).bottom(sc(GUTTER_B));
        for db in eqgraph::db_ticks(scale) {
            db_labels = db_labels.child(
                div()
                    .absolute()
                    .right(sc(6.))
                    .top(relative(eqgraph::frac_of_db(db, scale)))
                    .mt(sc(-6.))
                    .h(sc(12.))
                    .child(label().child(eqgraph::db_label(db))),
            );
        }
        // Frequencies along the bottom margin, centred under their grid lines.
        let mut freq_labels = div().absolute().left(sc(GUTTER_L)).right(sc(GUTTER_R)).bottom(sc(0.)).h(sc(GUTTER_B));
        for (freq, major) in eqgraph::grid_freqs() {
            if major {
                freq_labels = freq_labels.child(
                    div()
                        .absolute()
                        .top(sc(5.))
                        .left(relative(eqgraph::frac_of_freq(freq)))
                        .ml(sc(-18.))
                        .w(sc(36.))
                        .flex()
                        .justify_center()
                        .child(label().child(eqgraph::freq_label(freq))),
                );
            }
        }
        let mut graph = div()
            .id("eq-graph")
            .relative()
            .flex_1()
            .min_w(sc(0.))
            .h(sc(GRAPH_HEIGHT))
            .rounded(sc(8.))
            .bg(theme::ink(0.03))
            .opacity(if s.eq_enabled { 1.0 } else { 0.5 })
            .child(plot)
            .child(db_labels)
            .child(freq_labels);
        if differ {
            graph = graph.child(
                div()
                    .absolute()
                    .top(sc(3.))
                    .right(sc(GUTTER_R + 4.))
                    .flex()
                    .gap(sc(10.))
                    .text_size(sc(11.))
                    .child(div().text_color(theme::accent()).child(crate::i18n::t("eq.channel_left")))
                    .child(div().text_color(theme::gold()).child(crate::i18n::t("eq.channel_right"))),
            );
        }
        let meters = self.eq_meters(cx);
        div().flex().gap(sc(10.)).items_start().child(graph).child(meters).into_any_element()
    }

    /// The vertical scale choice of the response graph.
    fn eq_scale_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.eq_scale;
        let pill = |id: &'static str, text: &'static str, value: Option<f32>, on: bool, cx: &mut Context<Self>| {
            small_button(id, text, on).on_click(cx.listener(move |this, _, _, cx| {
                this.eq_scale = value;
                cx.notify();
            }))
        };
        div()
            .px(sc(super::widgets::PANEL_PAD - 4.))
            .py(sc(6.))
            .flex()
            .items_center()
            .gap(sc(8.))
            .child(pill("eq-scale-auto", crate::i18n::t("eq.scale_auto"), None, current.is_none(), cx))
            .child(pill("eq-scale-6", "±6", Some(6.0), current == Some(6.0), cx))
            .child(pill("eq-scale-12", "±12", Some(12.0), current == Some(12.0), cx))
            .child(pill("eq-scale-24", "±24", Some(24.0), current == Some(24.0), cx))
            .into_any_element()
    }

    /// A vertical slider (top is 1.0), like the horizontal one but dragged by height.
    fn eq_vslider(&mut self, key: usize, frac: f32, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        div()
            .id(("eq-vslider", key))
            .flex_none()
            .w(sc(22.))
            .h_full()
            .relative()
            .cursor_pointer()
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
            .child(div().absolute().left(sc(9.)).w(sc(4.)).top(sc(0.)).bottom(sc(0.)).rounded(sc(2.)).bg(theme::ink(0.18)))
            // 0 dB, in the middle.
            .child(div().absolute().left(sc(4.)).w(sc(14.)).top(relative(0.5)).h(sc(1.)).bg(theme::ink(0.4)))
            .child(
                div()
                    .absolute()
                    .left(sc(3.))
                    .w(sc(16.))
                    .h(sc(10.))
                    .top(relative(1.0 - frac.clamp(0.0, 1.0)))
                    .mt(sc(-5.))
                    .rounded(sc(3.))
                    .bg(theme::text()),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    if ev.click_count >= 2 {
                        // A double click puts the channel back to 0 dB.
                        this.eq_set_trim(key - KEY_TRIM, 0.0, true, cx);
                        return;
                    }
                    this.drag = Some(Drag::EqV(key));
                    super::winctl::capture_mouse(window);
                    this.drag_to(ev.position, cx);
                }),
            )
            .into_any_element()
    }

    /// The level meters and the channel gain faders, to the right of the graph.
    fn eq_meters(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use super::eqmeter;
        let trims = self.settings.lock().eq_trim;
        let colors = [gpui::Hsla::from(theme::accent()), gpui::Hsla::from(theme::gold()), gpui::Hsla::from(theme::danger())];
        let (track, mark) = (gpui::Hsla::from(theme::ink(0.07)), gpui::Hsla::from(theme::ink(0.2)));
        let small = || div().text_size(sc(10.)).text_color(theme::text_dim());
        // Scale marks, level with the bars.
        let mut scale = div().flex_none().w(sc(26.)).h_full().flex().flex_col().child(div().h(sc(GUTTER_T))).child({
            let mut marks = div().flex_1().relative();
            for tick in eqmeter::TICKS {
                marks = marks.child(
                    div()
                        .absolute()
                        .right(sc(3.))
                        .top(relative(1.0 - eqmeter::frac(tick)))
                        .mt(sc(-6.))
                        .h(sc(12.))
                        .child(small().child(format!("{tick:.0}"))),
                );
            }
            marks
        });
        scale = scale.child(div().h(sc(GUTTER_B)));
        let mut panel = div()
            .flex_none()
            .w(sc(METER_PANEL_W))
            .h(sc(GRAPH_HEIGHT))
            .rounded(sc(8.))
            .bg(theme::ink(0.03))
            .flex()
            .items_start()
            .child(scale);
        for (ch, name) in [(0usize, crate::i18n::t("eq.channel_left")), (1, crate::i18n::t("eq.channel_right"))] {
            let (level, hold) = (self.eq_meter.level_db(ch), self.eq_meter.hold_db(ch));
            let clip = self.eq_meter.clip(ch);
            let bar = div().flex_none().w(sc(14.)).h_full().child(
                canvas(|bounds, _, _| bounds, move |bounds, _, window, _| eqmeter::paint(bounds, level, hold, colors, track, mark, window))
                    .size_full(),
            );
            let fader = self.eq_vslider(KEY_TRIM + ch, eqmeter::frac_of_trim(trims[ch]), cx);
            // Name and the over-full-scale light (click to clear).
            let head = div().h(sc(GUTTER_T)).flex().items_center().gap(sc(4.)).child(small().child(name)).child(
                div()
                    .id(("eq-clip", ch))
                    .size(sc(8.))
                    .rounded(sc(4.))
                    .bg(if clip { theme::danger() } else { theme::ink(0.12) })
                    .cursor_pointer()
                    .tooltip(super::sidebar::tip(crate::i18n::t("eq.clip_tip")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.eq_meter.clear_clip(ch);
                        cx.notify();
                    })),
            );
            let readout = div().h(sc(GUTTER_B)).flex().items_center().child(small().child(format!("{:+.1}", trims[ch])));
            panel = panel.child(
                div()
                    .id(("eq-trim-column", ch))
                    .flex_none()
                    .w(sc(54.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .tooltip(super::sidebar::tip(crate::i18n::t("eq.trim_tip")))
                    .child(head)
                    .child(div().flex_1().flex().gap(sc(4.)).child(bar).child(fader))
                    .child(readout),
            );
        }
        panel.into_any_element()
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
        self.eq_start_meter(cx);
        let graph = self.eq_graph(cx);
        let scale_row = self.eq_scale_row(cx);

        // ------------------------------------------------ switch, status, pre-amp
        let mut status = if active { crate::i18n::t("eq.status_active").to_string() } else { crate::i18n::t("eq.status_idle").to_string() };
        if let Some(db) = self.eq_meter.loudest_db() {
            // What really reaches the device: over 0 dBFS the output is clipping.
            let key = if db > 0.0 { "eq.clip" } else { "eq.peak" };
            status = format!("{status}  |  {}", crate::i18n::text(key, &[format!("{db:.1}")]));
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
        } else {
            chain.push(self.eq_header());
        }
        for (i, f) in s.eq_filters.iter().enumerate() {
            let Some(r) = self.eq_rows.get(i) else { continue };
            let (freq, gain, q) = (r.freq.clone(), r.gain.clone(), r.q.clone());
            let on = f.enabled;
            let kind = f.kind;
            let has_gain = kind.has_gain();
            let open = self.eq_type_open == Some(i);
            let selected = self.eq_selected == Some(i);
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
            // One line per filter, like the list of Equalizer APO: on, number, type,
            // channel, frequency, gain, Q and the row's actions. Every value is visible.
            let mut item = div()
                .id(("eq-filter", i))
                .px(sc(super::widgets::PANEL_PAD - 4.))
                .py(sc(6.))
                .flex()
                .flex_col()
                .gap(sc(6.))
                .cursor_pointer()
                .rounded(sc(8.))
                .bg(theme::ink(if selected { 0.09 } else { 0.0 }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.eq_selected = Some(i);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x(sc(8.))
                        .gap_y(sc(6.))
                        .opacity(if on { 1.0 } else { 0.55 })
                        .child(div().w(sc(COL_ON)).flex().child(toggle))
                        .child(div().w(sc(COL_NUM)).text_size(sc(12.)).text_color(theme::text_secondary()).child(format!("{}", i + 1)))
                        .child(div().w(sc(COL_TYPE)).child(kind_button))
                        .child(div().w(sc(COL_CHANNEL)).child(channel_button))
                        .child(self.eq_cell(&freq, COL_FREQ, true))
                        .child(self.eq_cell(&gain, COL_GAIN, has_gain))
                        .child(self.eq_cell(&q, COL_Q, true))
                        .child(div().flex_1())
                        .child(up)
                        .child(down)
                        .child(remove),
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
        if let Some((i, f)) = self.eq_selected.and_then(|i| s.eq_filters.get(i).map(|f| (i, *f))) {
            chain.push(self.eq_detail(i, &f, cx));
        } else if count > 0 {
            chain.push(row("eq-hint", crate::i18n::t("eq.select_hint"), None, None).text_color(theme::text_secondary()).into_any_element());
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
            .child(group(Icon::Stats, crate::i18n::t("eq.graph"), crate::i18n::t("eq.graph_help"), vec![scale_row, graph]))
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

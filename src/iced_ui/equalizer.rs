//! The equaliser page: the chain of filters or the graphic bars, their
//! numeric fields, the response graph with draggable nodes and the level
//! meters. Same rules as the GPUI reference (`src/ui/equalizer.rs`).

use iced::widget::{button, canvas, column, container, mouse_area, pick_list, row, slider, text, text_input, vertical_slider, Space};
use iced::{alignment, mouse, Alignment, Border, Color, Element, Length, Point, Size};

use super::icons::{icon, Icon};
use super::settings::{group, setting, small_button};
use super::widgets::{self, switch};
use super::{theme, App, Message};
use crate::audio::dsp::{Channels, FilterKind, FilterSpec, MAX_FILTERS, MAX_FREQ, MAX_GAIN_DB, MAX_PREAMP_DB, MIN_FREQ, MIN_PREAMP_DB};
use crate::audio::graphic::{Band, EqMode};
use crate::shared::{eqgraph, eqmeter};

const GAIN_SLIDER_DB: f32 = 20.0;
const DEFAULT_RATE: u32 = 48_000;
/// How near a click must be to take hold of a node (px).
const NODE_REACH: f32 = 14.0;
const GRAPH_H: f32 = 220.0;

/// A numeric field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Preamp,
    Freq(usize),
    Gain(usize),
    Q(usize),
    BandFreq,
    BandGain,
}

#[derive(Debug, Clone)]
pub enum EqMsg {
    Enabled(bool),
    Mode(EqMode),
    Auto(bool),
    Preamp(f32),
    Typed(Field, String),
    Submit(Field),
    Scale(Option<f32>),
    Add,
    AddAt(f32, f32),
    Remove(usize),
    Move(usize, bool),
    Toggle(usize),
    Kind(usize, FilterKind),
    Channel(usize),
    Node(usize, f32, f32),
    Wheel(f32),
    Freq(usize, f32),
    Gain(usize, f32),
    Q(usize, f32),
    Select(usize),
    Commit,
    Reset,
    Band(usize, f32),
    BandSelect(usize),
    BandAdd,
    BandRemove,
    BandPreset(usize),
    BandsFlat,
    Trim(usize, f32),
    TrimReset(usize),
    ClearClip(usize),
    Meter,
}

fn round_tenth(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

fn fmt_freq(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// A typed number, with either decimal separator.
fn parse(text: &str) -> Option<f32> {
    text.trim().replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite())
}

/// The middle, on the log axis, of the widest gap between bars.
fn widest_gap(freqs: &[f32]) -> f32 {
    let mut edges = vec![MIN_FREQ];
    edges.extend_from_slice(freqs);
    edges.push(MAX_FREQ);
    let (lo, hi) =
        edges.windows(2).map(|w| (w[0], w[1])).max_by(|a, b| (a.1 / a.0).total_cmp(&(b.1 / b.0))).unwrap_or((MIN_FREQ, MAX_FREQ));
    (lo * hi).sqrt().round().clamp(MIN_FREQ, MAX_FREQ)
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

/// A filter type as a pick-list option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KindOption(FilterKind);

impl std::fmt::Display for KindOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(kind_label(self.0))
    }
}

impl App {
    /// Publish the chain to the audio thread; `save` also writes `settings.json`.
    pub(super) fn eq_apply(&mut self, save: bool) {
        let rate = if self.snap.output_rate > 0 { self.snap.output_rate } else { DEFAULT_RATE };
        let cfg = {
            let mut s = self.settings.lock();
            if s.eq_auto_preamp {
                // Measured with the switch on, so the number is right the moment it is turned on.
                let mut measured = s.eq_config();
                measured.enabled = true;
                s.eq_preamp = measured.auto_preamp(f64::from(rate));
            }
            if save {
                s.save();
            }
            s.eq_config()
        };
        self.player.set_eq(&cfg);
    }

    pub(super) fn eq_start(&mut self) {
        self.eq_apply(false);
    }

    /// The meters and their faster refresh are only on while the page shows.
    pub(super) fn eq_visible(&self) -> bool {
        !self.now_playing_open && self.page == crate::shared::Page::Settings && self.settings_tab == super::settings::EQ_TAB
    }

    fn eq_meter_step(&mut self) {
        let dt = self.eq_meter_at.elapsed().as_secs_f32().min(0.25);
        self.eq_meter_at = std::time::Instant::now();
        let peaks = self.player.take_peaks();
        self.eq_meter.update(peaks, dt);
    }

    pub(super) fn eq_update(&mut self, msg: EqMsg) {
        let mut save = true;
        let mut bands_changed = false;
        match &msg {
            EqMsg::Meter => return self.eq_meter_step(),
            EqMsg::ClearClip(ch) => return self.eq_meter.clear_clip(*ch),
            EqMsg::Scale(scale) => {
                self.eq_scale = *scale;
                return;
            }
            EqMsg::Typed(field, value) => {
                self.eq_draft = Some((*field, value.clone()));
                return;
            }
            EqMsg::Select(i) => {
                self.eq_selected = (self.eq_selected != Some(*i)).then_some(*i);
                self.eq_draft = None;
                return;
            }
            EqMsg::BandSelect(i) => {
                self.eq_band_selected = Some(*i);
                self.eq_draft = None;
                return;
            }
            _ => {}
        }
        // Whatever is applied now is what the fields show.
        let draft = self.eq_draft.take();
        {
            let mut s = self.settings.lock();
            let full = s.eq_filters.len() >= MAX_FILTERS;
            match msg {
                EqMsg::Enabled(on) => s.eq_enabled = on,
                EqMsg::Mode(mode) => s.eq_mode = mode,
                EqMsg::Auto(on) => s.eq_auto_preamp = on,
                EqMsg::Preamp(db) => {
                    // With the automatic pre-amp on, the number is not for moving.
                    if !s.eq_auto_preamp {
                        s.eq_preamp = round_tenth(db).clamp(MIN_PREAMP_DB, MAX_PREAMP_DB);
                    }
                    save = false;
                }
                EqMsg::Submit(field) => {
                    let typed = draft.filter(|(f, _)| *f == field).and_then(|(_, text)| parse(&text));
                    let Some(v) = typed else { return };
                    match field {
                        Field::Preamp => {
                            if !s.eq_auto_preamp {
                                s.eq_preamp = round_tenth(v).clamp(MIN_PREAMP_DB, MAX_PREAMP_DB);
                            }
                        }
                        Field::Freq(i) | Field::Gain(i) | Field::Q(i) => {
                            if let Some(f) = s.eq_filters.get_mut(i) {
                                match field {
                                    Field::Freq(_) => f.freq = v,
                                    Field::Gain(_) => f.gain_db = v,
                                    _ => f.q = v,
                                }
                                *f = f.clamped();
                            }
                        }
                        Field::BandFreq | Field::BandGain => {
                            let Some(i) = self.eq_band_selected else { return };
                            let Some(band) = s.eq_bands.get_mut(i) else { return };
                            if field == Field::BandFreq {
                                band.freq = v.clamp(MIN_FREQ, MAX_FREQ);
                            } else {
                                band.gain_db = round_tenth(v).clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
                            }
                            // A new frequency may change the bar's place: the selection follows it.
                            let moved = *band;
                            s.eq_bands.sort_by(|a, b| a.freq.total_cmp(&b.freq));
                            self.eq_band_selected = s.eq_bands.iter().position(|b| *b == moved);
                            bands_changed = true;
                        }
                    }
                }
                EqMsg::Add | EqMsg::AddAt(..) if full => {
                    drop(s);
                    self.notify_toast(crate::i18n::t("eq.full"));
                    return;
                }
                EqMsg::Add => {
                    s.eq_filters.push(FilterSpec::new(FilterKind::Peaking));
                    self.eq_selected = Some(s.eq_filters.len() - 1);
                }
                EqMsg::AddAt(freq, gain) => {
                    let mut filter = FilterSpec::new(FilterKind::Peaking);
                    filter.freq = freq;
                    filter.gain_db = gain;
                    s.eq_filters.push(filter.clamped());
                    self.eq_selected = Some(s.eq_filters.len() - 1);
                }
                EqMsg::Remove(i) => {
                    if i < s.eq_filters.len() {
                        s.eq_filters.remove(i);
                    }
                    self.eq_selected = match self.eq_selected {
                        Some(sel) if sel == i => None,
                        Some(sel) if sel > i => Some(sel - 1),
                        other => other,
                    };
                }
                EqMsg::Move(i, down) => {
                    let j = if down { i + 1 } else { i.wrapping_sub(1) };
                    if i >= s.eq_filters.len() || j >= s.eq_filters.len() {
                        return;
                    }
                    s.eq_filters.swap(i, j);
                    // The selection follows the filter that moved.
                    if self.eq_selected == Some(i) {
                        self.eq_selected = Some(j);
                    } else if self.eq_selected == Some(j) {
                        self.eq_selected = Some(i);
                    }
                }
                EqMsg::Toggle(i) => {
                    if let Some(f) = s.eq_filters.get_mut(i) {
                        f.enabled = !f.enabled;
                    }
                }
                EqMsg::Kind(i, kind) => {
                    if let Some(f) = s.eq_filters.get_mut(i) {
                        // Between a gain filter and a pass filter the usual Q changes (1 / Butterworth).
                        if f.kind.has_gain() != kind.has_gain() && (f.q - FilterSpec::new(f.kind).q).abs() < 0.005 {
                            f.q = FilterSpec::new(kind).q;
                        }
                        f.kind = kind;
                    }
                }
                EqMsg::Channel(i) => {
                    if let Some(f) = s.eq_filters.get_mut(i) {
                        f.channels = f.channels.next();
                    }
                }
                EqMsg::Node(i, freq, gain) => {
                    let Some(f) = s.eq_filters.get_mut(i) else { return };
                    f.freq = freq;
                    if f.kind.has_gain() {
                        f.gain_db = gain;
                    }
                    *f = f.clamped();
                    self.eq_selected = Some(i);
                    save = false;
                }
                EqMsg::Wheel(dy) => {
                    let Some(f) = self.eq_selected.and_then(|i| s.eq_filters.get_mut(i)) else { return };
                    f.q = ((f.q * (dy / 240.0).exp() * 100.0).round() / 100.0).clamp(eqgraph::Q_RANGE.0, eqgraph::Q_RANGE.1);
                    *f = f.clamped();
                }
                EqMsg::Freq(i, frac) => {
                    if let Some(f) = s.eq_filters.get_mut(i) {
                        f.freq = (eqgraph::freq_of_frac(f64::from(frac)) as f32).round().clamp(MIN_FREQ, MAX_FREQ);
                    }
                    save = false;
                }
                EqMsg::Gain(i, db) => {
                    if let Some(f) = s.eq_filters.get_mut(i) {
                        f.gain_db = round_tenth(db).clamp(-MAX_GAIN_DB, MAX_GAIN_DB);
                    }
                    save = false;
                }
                EqMsg::Q(i, frac) => {
                    if let Some(f) = s.eq_filters.get_mut(i) {
                        f.q = eqgraph::q_of_frac(frac);
                    }
                    save = false;
                }
                EqMsg::Reset => {
                    s.eq_filters.clear();
                    s.eq_preamp = 0.0;
                    self.eq_selected = None;
                }
                EqMsg::Band(i, db) => {
                    if let Some(band) = s.eq_bands.get_mut(i) {
                        band.gain_db = if db.abs() < 0.25 { 0.0 } else { round_tenth(db).clamp(-MAX_GAIN_DB, MAX_GAIN_DB) };
                    }
                    self.eq_band_selected = Some(i);
                    bands_changed = true;
                    save = false;
                }
                EqMsg::BandAdd => {
                    if s.eq_bands.len() >= MAX_FILTERS {
                        drop(s);
                        self.notify_toast(crate::i18n::t("eq.full"));
                        return;
                    }
                    let freqs: Vec<f32> = s.eq_bands.iter().map(|b| b.freq).collect();
                    let freq = widest_gap(&freqs);
                    s.eq_bands.push(Band::new(freq));
                    s.eq_bands.sort_by(|a, b| a.freq.total_cmp(&b.freq));
                    self.eq_band_selected = s.eq_bands.iter().position(|b| b.freq == freq);
                    bands_changed = true;
                }
                EqMsg::BandRemove => {
                    let Some(i) = self.eq_band_selected else { return };
                    if s.eq_bands.len() <= 1 || i >= s.eq_bands.len() {
                        return;
                    }
                    s.eq_bands.remove(i);
                    self.eq_band_selected = Some(i.min(s.eq_bands.len() - 1));
                    bands_changed = true;
                }
                EqMsg::BandPreset(count) => {
                    s.eq_bands = crate::audio::graphic::iso_bands(count);
                    self.eq_band_selected = None;
                    bands_changed = true;
                }
                EqMsg::BandsFlat => {
                    for band in &mut s.eq_bands {
                        band.gain_db = 0.0;
                    }
                    bands_changed = true;
                }
                EqMsg::Trim(ch, db) => {
                    let db = if db.abs() < 0.25 { 0.0 } else { round_tenth(db) };
                    s.eq_trim[ch] = db.clamp(-eqmeter::TRIM_RANGE_DB, eqmeter::TRIM_RANGE_DB);
                    save = false;
                }
                EqMsg::TrimReset(ch) => s.eq_trim[ch] = 0.0,
                EqMsg::Commit
                | EqMsg::Meter
                | EqMsg::ClearClip(_)
                | EqMsg::Scale(_)
                | EqMsg::Typed(..)
                | EqMsg::Select(_)
                | EqMsg::BandSelect(_) => {}
            }
            if bands_changed {
                s.recompile_bands();
            }
        }
        self.eq_apply(save);
    }
}

// ----------------------------------------------------------------- graph

/// The response graph: grid, both channels' curves and the filters' nodes.
/// Drag a node to move it, click empty space to add a filter, right click
/// removes, the wheel changes the selected filter's Q.
struct Graph {
    left: Vec<f32>,
    right: Vec<f32>,
    scale: f32,
    filters: Vec<FilterSpec>,
    selected: Option<usize>,
    editable: bool,
}

#[derive(Default)]
struct GraphState {
    /// The node held, and the scale when it was taken (it must not move under the pointer).
    held: Option<(usize, f32)>,
}

impl Graph {
    fn scale_now(&self, state: &GraphState) -> f32 {
        state.held.map_or(self.scale, |(_, scale)| scale)
    }

    fn point(&self, f: &FilterSpec, scale: f32) -> (f32, f32) {
        eqgraph::node_pos(f.freq, f.gain_db, f.kind.has_gain(), scale)
    }
}

impl canvas::Program<Message> for Graph {
    type State = GraphState;

    fn update(
        &self,
        state: &mut GraphState,
        event: &canvas::Event,
        bounds: iced::Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if !self.editable {
            return None;
        }
        let (w, h) = (bounds.width.max(1.0), bounds.height.max(1.0));
        let fracs = |p: Point| (((p.x - bounds.x) / w).clamp(0.0, 1.0), ((p.y - bounds.y) / h).clamp(0.0, 1.0));
        let publish = |msg: EqMsg| canvas::Action::publish(Message::Eq(msg));
        let iced::Event::Mouse(event) = event else { return None };
        match event {
            mouse::Event::ButtonPressed(pressed @ (mouse::Button::Left | mouse::Button::Right)) => {
                let at = fracs(cursor.position_over(bounds)?);
                let points: Vec<(f32, f32)> = self.filters.iter().map(|f| self.point(f, self.scale)).collect();
                let hit = eqgraph::nearest_node(&points, at, (w, h), NODE_REACH);
                if *pressed == mouse::Button::Right {
                    return Some(publish(EqMsg::Remove(hit?)).and_capture());
                }
                let (freq, gain) = eqgraph::freq_gain_at(at.0, at.1, self.scale);
                let (held, msg) = match hit {
                    Some(i) => (Some(i), EqMsg::Node(i, self.filters[i].freq, self.filters[i].gain_db)),
                    None => ((self.filters.len() < MAX_FILTERS).then_some(self.filters.len()), EqMsg::AddAt(freq, gain)),
                };
                state.held = held.map(|i| (i, self.scale));
                Some(publish(msg).and_capture())
            }
            mouse::Event::CursorMoved { position } => {
                let (i, scale) = state.held?;
                let (fx, fy) = fracs(*position);
                let (freq, gain) = eqgraph::freq_gain_at(fx, fy, scale);
                Some(publish(EqMsg::Node(i, freq, gain)).and_capture())
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                state.held.take()?;
                Some(publish(EqMsg::Commit).and_capture())
            }
            mouse::Event::WheelScrolled { delta } => {
                cursor.position_over(bounds)?;
                self.selected?;
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 20.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                Some(publish(EqMsg::Wheel(dy)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &GraphState,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let scale = self.scale_now(state);
        let (grid, zero) = (theme::ink(0.10), theme::ink(0.28));
        let label = |content: String, position: Point| canvas::Text {
            content,
            position,
            color: theme::text_dim(),
            size: 10.0.into(),
            font: super::INTER,
            ..canvas::Text::default()
        };
        for (freq, major) in eqgraph::grid_freqs() {
            let x = (eqgraph::frac_of_freq(freq) * w).min(w - 1.0);
            let color = if major { grid } else { theme::with_alpha(grid, grid.a * 0.5) };
            frame.fill_rectangle(Point::new(x, 0.0), Size::new(1.0, h), color);
            if major {
                frame.fill_text(label(eqgraph::freq_label(freq), Point::new((x + 3.0).min(w - 20.0), h - 14.0)));
            }
        }
        for db in eqgraph::db_ticks(scale) {
            let y = eqgraph::frac_of_db(db, scale) * h;
            frame.fill_rectangle(Point::new(0.0, y), Size::new(w, 1.0), if db == 0.0 { zero } else { grid });
            frame.fill_text(label(eqgraph::db_label(db), Point::new(4.0, (y + 2.0).min(h - 28.0))));
        }
        for (curve, color) in [(&self.right, theme::with_alpha(theme::gold(), 0.8)), (&self.left, theme::accent())] {
            if curve.len() < 2 {
                continue;
            }
            let path = canvas::Path::new(|p| {
                for (i, db) in curve.iter().enumerate() {
                    let at = Point::new(w * i as f32 / (curve.len() - 1) as f32, eqgraph::frac_of_db(*db, scale) * h);
                    if i == 0 {
                        p.move_to(at);
                    } else {
                        p.line_to(at);
                    }
                }
            });
            frame.stroke(&path, canvas::Stroke::default().with_width(2.0).with_color(color));
        }
        let accent = theme::accent();
        for (i, filter) in self.filters.iter().enumerate() {
            let selected = self.selected == Some(i);
            let (fx, fy) = self.point(filter, scale);
            let node = canvas::Path::circle(Point::new(w * fx, h * fy), if selected { 7.0 } else { 5.0 });
            frame.fill(&node, theme::with_alpha(accent, if selected { 0.9 } else { 0.18 }));
            let ring = theme::with_alpha(accent, if filter.enabled { 1.0 } else { 0.4 });
            frame.stroke(&node, canvas::Stroke::default().with_width(2.0).with_color(ring));
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &GraphState, bounds: iced::Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if !self.editable {
            mouse::Interaction::default()
        } else if state.held.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

/// A level meter: bar in colour zones, scale marks, held peak.
struct MeterView {
    level: f32,
    hold: f32,
}

impl canvas::Program<Message> for MeterView {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), theme::ink(0.08));
        let colors = [theme::accent(), theme::gold(), theme::danger()];
        let top = eqmeter::frac(self.level);
        let mut from = 0.0f32;
        for (limit, color) in [(eqmeter::frac(-12.0), colors[0]), (eqmeter::frac(-3.0), colors[1]), (1.0, colors[2])] {
            let to = top.min(limit);
            if to > from {
                frame.fill_rectangle(Point::new(0.0, h * (1.0 - to)), Size::new(w, h * (to - from)), color);
                from = to;
            }
        }
        for tick in eqmeter::TICKS {
            frame.fill_rectangle(Point::new(0.0, h * (1.0 - eqmeter::frac(tick))), Size::new(w, 1.0), theme::ink(0.25));
        }
        if self.hold.is_finite() {
            let color = colors[eqmeter::zone(self.hold) as usize];
            frame.fill_rectangle(Point::new(0.0, h * (1.0 - eqmeter::frac(self.hold))), Size::new(w, 2.0), color);
        }
        vec![frame.into_geometry()]
    }
}

// ----------------------------------------------------------------- page

fn field<'a>(app: &App, which: Field, value: String, width: f32, enabled: bool) -> Element<'a, Message> {
    let shown = app.eq_draft.as_ref().filter(|(f, _)| *f == which).map_or(value, |(_, typed)| typed.clone());
    let mut input = text_input("", &shown).size(12).padding([5, 6]).width(width).style(super::pages::field_style);
    if enabled {
        input = input.on_input(move |v| Message::Eq(EqMsg::Typed(which, v))).on_submit(Message::Eq(EqMsg::Submit(which)));
    }
    input.into()
}

fn fine_slider<'a>(
    label: &'a str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    on: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    let bar = slider(range, value, on).step(0.001_f32).on_release(Message::Eq(EqMsg::Commit)).width(Length::Fill);
    row![text(label).size(12).color(theme::text_secondary()).width(110), bar].spacing(10).align_y(Alignment::Center).into()
}

fn arrow<'a>(glyph: Icon, enabled: bool, msg: EqMsg) -> Element<'a, Message> {
    let color = if enabled { theme::text_secondary() } else { theme::with_alpha(theme::text_dim(), 0.35) };
    let b = button(container(icon(glyph, 12.0, color)).center(22)).padding(0).style(widgets::still(5.0));
    if enabled {
        super::wash::wash(b.on_press(Message::Eq(msg)), theme::hover(), 5.0).press(theme::ink(0.06)).into()
    } else {
        b.into()
    }
}

fn note<'a>(s: &'a str) -> Element<'a, Message> {
    container(text(s).size(13).color(theme::text_secondary())).padding([12.0, super::settings::ROW_X]).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let s = app.settings.lock().clone();
    let cfg = s.eq_config();
    let graphic = s.eq_mode == EqMode::Graphic;
    let mut status = if cfg.is_bypass() { crate::i18n::t("eq.status_idle") } else { crate::i18n::t("eq.status_active") }.to_string();
    if let Some(db) = app.eq_meter.loudest_db() {
        // What really reaches the device: over 0 dBFS the output is clipping.
        let key = if db > 0.0 { "eq.clip" } else { "eq.peak" };
        status = format!("{status}  |  {}", crate::i18n::text(key, &[format!("{db:.1}")]));
    }
    let eq = |msg: EqMsg| Message::Eq(msg);
    let modes = row![
        small_button(crate::i18n::t("eq.mode_parametric"), !graphic, eq(EqMsg::Mode(EqMode::Parametric))),
        small_button(crate::i18n::t("eq.mode_graphic"), graphic, eq(EqMsg::Mode(EqMode::Graphic))),
    ]
    .spacing(8);
    let manual = !s.eq_auto_preamp;
    let displayed_preamp = if s.eq_preamp.abs() < 0.05 { 0.0 } else { s.eq_preamp };
    let preamp_slider = slider(MIN_PREAMP_DB..=MAX_PREAMP_DB, s.eq_preamp, move |v| Message::Eq(EqMsg::Preamp(v)))
        .step(0.1_f32)
        .width(180)
        .style(move |palette, status| {
            let mut style = slider::default(palette, status);
            if !manual {
                style.rail.backgrounds = (theme::ink(0.12).into(), theme::ink(0.08).into());
                style.handle.background = theme::ink(0.28).into();
            }
            style
        });
    let preamp_slider = if manual { preamp_slider.on_release(eq(EqMsg::Commit)) } else { preamp_slider };
    let preamp = row![preamp_slider, field(app, Field::Preamp, format!("{displayed_preamp:.1}"), 64.0, manual)]
        .spacing(8)
        .align_y(Alignment::Center);
    let main = vec![
        setting(
            crate::i18n::t("eq.enable"),
            Some(format!("{} {status}", crate::i18n::t("eq.enable_help"))),
            switch(s.eq_enabled, eq(EqMsg::Enabled(!s.eq_enabled))),
        ),
        setting(crate::i18n::t("eq.mode"), None, modes.into()),
        setting(
            crate::i18n::t("eq.auto_preamp"),
            Some(crate::i18n::t("eq.auto_preamp_help").to_string()),
            switch(s.eq_auto_preamp, eq(EqMsg::Auto(!s.eq_auto_preamp))),
        ),
        setting(crate::i18n::t("eq.preamp"), Some(crate::i18n::t("eq.preamp_help").to_string()), preamp.into()),
    ];
    let chain = if graphic {
        group(Icon::Library, crate::i18n::t("eq.bars"), crate::i18n::t("eq.bars_description"), bars(app, &s.eq_bands))
    } else {
        group(Icon::Library, crate::i18n::t("eq.filters"), crate::i18n::t("eq.filters_description"), filters(app, &s.eq_filters))
    };
    column![
        group(Icon::VolumeUp, crate::i18n::t("eq.title"), crate::i18n::t("eq.description"), main),
        group(Icon::Stats, crate::i18n::t("eq.graph"), crate::i18n::t("eq.graph_help"), vec![response(app, &s)]),
        chain,
    ]
    .spacing(super::settings::GROUP_GAP)
    .into()
}

fn response<'a>(app: &App, s: &crate::config::Settings) -> Element<'a, Message> {
    let rate = f64::from(if app.snap.output_rate > 0 { app.snap.output_rate } else { DEFAULT_RATE });
    let mut cfg = s.eq_config();
    cfg.enabled = true;
    let freqs = eqgraph::freqs();
    let (left, right) = (cfg.response_curve(0, rate, &freqs), cfg.response_curve(1, rate, &freqs));
    let scale = app.eq_scale.unwrap_or_else(|| eqgraph::scale_for(&[&left, &right]));
    let editable = s.eq_mode == EqMode::Parametric;
    let graph = canvas(Graph {
        left,
        right,
        scale,
        filters: if editable { s.eq_filters.clone() } else { Vec::new() },
        selected: app.eq_selected,
        editable,
    })
    .width(Length::Fill)
    .height(GRAPH_H);

    let mut choices =
        row![small_button(crate::i18n::t("eq.scale_auto"), app.eq_scale.is_none(), Message::Eq(EqMsg::Scale(None)))].spacing(6);
    for db in [6.0_f32, 12.0, 24.0] {
        choices = choices.push(small_button(format!("±{db:.0}"), app.eq_scale == Some(db), Message::Eq(EqMsg::Scale(Some(db)))));
    }

    let mut meters = row![].spacing(10);
    for ch in 0..2usize {
        let clipped = app.eq_meter.clip(ch);
        let lamp = button(Space::new().width(10).height(6))
            .padding(0)
            .style(move |_, _| button::Style {
                background: Some(if clipped { theme::danger() } else { theme::ink(0.12) }.into()),
                border: Border { radius: 2.0.into(), ..Default::default() },
                ..Default::default()
            })
            .on_press(Message::Eq(EqMsg::ClearClip(ch)));
        let clip = iced::widget::tooltip(lamp, text(crate::i18n::t("eq.clip_tip")).size(11), iced::widget::tooltip::Position::Top)
            .style(widgets::panel_style)
            .padding(6);
        let level = canvas(MeterView { level: app.eq_meter.level_db(ch), hold: app.eq_meter.hold_db(ch) }).width(10).height(GRAPH_H - 28.0);
        let range = eqmeter::TRIM_RANGE_DB;
        let fader = vertical_slider(-range..=range, s.eq_trim[ch], move |v| Message::Eq(EqMsg::Trim(ch, v)))
            .step(0.1_f32)
            .on_release(Message::Eq(EqMsg::Commit))
            .height(GRAPH_H - 28.0)
            // Each fader in its channel's colour, as the legend and its value.
            .style(move |palette, status| {
                let tone = if ch == 0 { theme::accent() } else { theme::gold() };
                let mut style = vertical_slider::default(palette, status);
                style.rail.backgrounds = (tone.into(), theme::ink(0.12).into());
                style.handle.background = tone.into();
                style
            });
        // The value under the fader puts it back at 0 dB when clicked.
        let value = text(format!("{:+.1}", s.eq_trim[ch])).size(10).color(if ch == 0 { theme::accent() } else { theme::gold() });
        let reset = mouse_area(value).on_press(Message::Eq(EqMsg::TrimReset(ch))).interaction(mouse::Interaction::Pointer);
        let reset = iced::widget::tooltip(reset, text(crate::i18n::t("eq.trim_tip")).size(11), iced::widget::tooltip::Position::Bottom)
            .style(widgets::panel_style)
            .padding(6);
        meters = meters.push(column![clip, row![level, fader].spacing(4), reset].spacing(5).align_x(Alignment::Center));
    }
    let key = |label: &'static str, color: Color| text(label).size(11).color(color);
    let legend =
        row![key(crate::i18n::t("eq.channel_left"), theme::accent()), key(crate::i18n::t("eq.channel_right"), theme::gold())].spacing(12);
    column![choices.wrap(), row![graph, meters].spacing(12), legend].spacing(12).padding([16.0, super::settings::ROW_X]).into()
}

fn filters<'a>(app: &App, list: &[FilterSpec]) -> Vec<Element<'a, Message>> {
    let mut out: Vec<Element<'a, Message>> = Vec::new();
    if list.is_empty() {
        out.push(note(crate::i18n::t("eq.empty")));
    } else {
        let title = |label: &'static str, width: f32| text(label).size(11).font(widgets::semibold()).color(theme::text_dim()).width(width);
        let columns = row![
            Space::new().width(38),
            title(crate::i18n::t("eq.col_type"), 124.0),
            title(crate::i18n::t("eq.col_channel"), 76.0),
            title(crate::i18n::t("eq.col_freq"), 66.0),
            title(crate::i18n::t("eq.col_gain"), 56.0),
            title(crate::i18n::t("eq.col_q"), 54.0),
        ]
        .spacing(8)
        .padding([8.0, super::settings::ROW_X]);
        out.push(columns.into());
    }
    let count = list.len();
    for (i, f) in list.iter().enumerate() {
        let selected = app.eq_selected == Some(i);
        let kinds = pick_list(FilterKind::ALL.map(KindOption).to_vec(), Some(KindOption(f.kind)), move |k: KindOption| {
            Message::Eq(EqMsg::Kind(i, k.0))
        })
        .text_size(12.0)
        .menu_style(super::settings::picker_menu_style)
        .width(124);
        let line = row![
            switch(f.enabled, Message::Eq(EqMsg::Toggle(i))),
            kinds,
            container(small_button(channel_label(f.channels), false, Message::Eq(EqMsg::Channel(i)))).width(76),
            field(app, Field::Freq(i), fmt_freq(f.freq), 66.0, true),
            field(app, Field::Gain(i), format!("{:.1}", f.gain_db), 56.0, f.kind.has_gain()),
            field(app, Field::Q(i), format!("{:.2}", f.q), 54.0, true),
            Space::new().width(Length::Fill),
            super::widgets::tip(arrow(Icon::ChevUp, i > 0, EqMsg::Move(i, false)), crate::i18n::t("eq.move_up")),
            super::widgets::tip(arrow(Icon::ChevDown, i + 1 < count, EqMsg::Move(i, true)), crate::i18n::t("eq.move_down")),
            super::widgets::tip(arrow(Icon::Close, true, EqMsg::Remove(i)), crate::i18n::t("eq.remove")),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let mut body = column![line].spacing(8);
        if selected {
            let freq = eqgraph::frac_of_freq(f64::from(f.freq));
            body = body.push(fine_slider(crate::i18n::t("eq.freq"), freq, 0.0..=1.0, move |v| Message::Eq(EqMsg::Freq(i, v))));
            if f.kind.has_gain() {
                let range = -GAIN_SLIDER_DB..=GAIN_SLIDER_DB;
                body = body.push(fine_slider(crate::i18n::t("eq.gain"), f.gain_db, range, move |v| Message::Eq(EqMsg::Gain(i, v))));
            }
            body = body.push(fine_slider(crate::i18n::t("eq.q"), eqgraph::frac_of_q(f.q), 0.0..=1.0, move |v| Message::Eq(EqMsg::Q(i, v))));
        }
        let boxed = container(body)
            .padding([8.0, super::settings::ROW_X])
            .style(move |_| container::Style { background: selected.then(|| theme::hover().into()), ..Default::default() });
        out.push(
            mouse_area(super::wash::wash(boxed, theme::ink(0.05), 6.0))
                .on_press(Message::Eq(EqMsg::Select(i)))
                .interaction(mouse::Interaction::Pointer)
                .into(),
        );
    }
    if !list.is_empty() && app.eq_selected.is_none() {
        out.push(note(crate::i18n::t("eq.select_hint")));
    }
    let footer = row![
        small_button(crate::i18n::t("eq.add"), true, Message::Eq(EqMsg::Add)),
        small_button(crate::i18n::t("eq.reset"), false, Message::Eq(EqMsg::Reset)),
        text(format!("{count} / {MAX_FILTERS}")).size(12).color(theme::text_dim()),
    ]
    .spacing(8)
    .padding([12.0, super::settings::ROW_X])
    .align_y(Alignment::Center);
    out.push(footer.into());
    out
}

fn bars<'a>(app: &App, bands: &[Band]) -> Vec<Element<'a, Message>> {
    let sliders = row(bands.iter().enumerate().map(|(i, band)| {
        let selected = app.eq_band_selected == Some(i);
        let color = if selected { theme::accent() } else { theme::text_dim() };
        let bar = vertical_slider(-MAX_GAIN_DB..=MAX_GAIN_DB, band.gain_db, move |v| Message::Eq(EqMsg::Band(i, v)))
            .step(0.1_f32)
            .on_release(Message::Eq(EqMsg::Commit))
            .height(180)
            // The whole range shows (the default leaves the rail above the
            // handle invisible on the dark card), as on the trim faders.
            .style(move |palette, status| {
                let mut style = vertical_slider::default(palette, status);
                let tone = if selected { theme::accent() } else { theme::with_alpha(theme::accent(), 0.75) };
                style.rail.backgrounds = (tone.into(), theme::ink(0.12).into());
                style.handle.background = tone.into();
                style
            });
        let name = mouse_area(text(eqgraph::freq_label(f64::from(band.freq))).size(10).color(color))
            .on_press(Message::Eq(EqMsg::BandSelect(i)))
            .interaction(mouse::Interaction::Pointer);
        column![text(format!("{:+.1}", band.gain_db)).size(10).color(theme::text_secondary()), bar, name]
            .spacing(6)
            .align_x(alignment::Horizontal::Center)
            .width(Length::Fill)
            .into()
    }))
    .spacing(4)
    .padding([12.0, super::settings::ROW_X]);
    let preset = |n: usize| small_button(n.to_string(), bands.len() == n, Message::Eq(EqMsg::BandPreset(n)));
    let tools = row![
        preset(10),
        preset(15),
        preset(31),
        small_button(crate::i18n::t("eq.bar_add"), true, Message::Eq(EqMsg::BandAdd)),
        small_button(crate::i18n::t("eq.bar_remove"), false, Message::Eq(EqMsg::BandRemove)),
        small_button(crate::i18n::t("eq.bars_reset"), false, Message::Eq(EqMsg::BandsFlat)),
    ]
    .spacing(8)
    .padding([12.0, super::settings::ROW_X]);
    let edit: Element<'a, Message> = match app.eq_band_selected.and_then(|i| bands.get(i)) {
        Some(band) => row![
            text(crate::i18n::t("eq.freq")).size(12).color(theme::text_secondary()),
            field(app, Field::BandFreq, fmt_freq(band.freq), 90.0, true),
            text(crate::i18n::t("eq.gain")).size(12).color(theme::text_secondary()),
            field(app, Field::BandGain, format!("{:.1}", band.gain_db), 70.0, true),
        ]
        .spacing(8)
        .padding([12.0, super::settings::ROW_X])
        .align_y(Alignment::Center)
        .into(),
        None => note(crate::i18n::t("eq.bars_hint")),
    };
    vec![sliders.into(), tools.wrap().into(), edit]
}

#[cfg(test)]
mod tests {
    use super::{parse, widest_gap};

    #[test]
    fn typed_numbers_take_either_decimal_separator_and_reject_the_rest() {
        assert_eq!(parse(" -3,5 "), Some(-3.5));
        assert_eq!(parse("1000"), Some(1000.0));
        for bad in ["", "abc", "inf", "NaN"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_new_bar_lands_in_the_middle_of_the_widest_gap() {
        let freq = widest_gap(&[40.0, 80.0, 8000.0, 16_000.0]);
        assert_eq!(freq, 800.0);
    }
}

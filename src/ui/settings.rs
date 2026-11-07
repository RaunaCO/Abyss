//! Settings page laid out like the official app: a title, General / Account /
//! About tabs, section headings and dark rounded cards whose rows have a bold
//! title, a grey description and the control on the right.

use super::sc;
use gpui::{div, prelude::*, AnyElement, Context, ElementId, FontWeight, SharedString};

use super::icons::Icon;
use super::{theme, AppView};
use crate::config::Quality;

const CARD_BG: u32 = 0x101010;

fn heading(text: &'static str) -> AnyElement {
    div().pt(sc(30.)).pb(sc(12.)).px(sc(12.)).text_size(sc(14.)).font_weight(FontWeight::BOLD).child(text).into_any_element()
}

/// A dark rounded card; rows are separated by hairlines.
fn card(rows: Vec<AnyElement>) -> AnyElement {
    div()
        .rounded(sc(12.))
        .bg(gpui::rgb(CARD_BG))
        .border_1()
        .border_color(gpui::rgba(0xffffff12))
        .overflow_hidden()
        .flex()
        .flex_col()
        .children(
            rows.into_iter().enumerate().map(|(i, r)| div().when(i > 0, |d| d.border_t_1().border_color(gpui::rgba(0xffffff0f))).child(r)),
        )
        .into_any_element()
}

fn row_text(title: impl Into<SharedString>, desc: Option<SharedString>) -> AnyElement {
    div()
        .flex_1()
        .min_w(sc(0.))
        .flex()
        .flex_col()
        .gap(sc(3.))
        .child(div().text_size(sc(12.)).font_weight(FontWeight::BOLD).child(title.into()))
        .when_some(desc, |d, t| d.child(div().text_size(sc(11.)).text_color(theme::text_secondary()).child(t)))
        .into_any_element()
}

fn row(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    desc: Option<SharedString>,
    right: Option<AnyElement>,
) -> gpui::Stateful<gpui::Div> {
    div().id(id).px(sc(16.)).py(sc(14.)).flex().items_center().gap(sc(16.)).child(row_text(title, desc)).children(right)
}

fn switch(on: bool, key: &str) -> AnyElement {
    // The knob slides and the track fades between colours.
    let v = super::interact::follow(&format!("sw-{key}"), if on { 1.0 } else { 0.0 }, 0.22);
    div()
        .flex_none()
        .w(sc(40.))
        .h(sc(22.))
        .rounded(sc(11.))
        .p(sc(2.))
        .bg(super::interact::mix(gpui::rgb(0x303036), gpui::rgb(0xe4e4e7), v))
        .flex()
        .child(div().ml(sc(18. * v)).size(sc(18.)).rounded(sc(9.)).bg(super::interact::mix(gpui::rgb(0x8a8a93), gpui::rgb(0x111111), v)))
        .into_any_element()
}

fn small_button(id: impl Into<ElementId>, label: impl Into<SharedString>, white: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(sc(28.))
        .px(sc(12.))
        .flex()
        .items_center()
        .gap(sc(6.))
        .rounded(sc(6.))
        .cursor_pointer()
        .text_size(sc(11.))
        .font_weight(FontWeight::BOLD)
        .bg(if white { gpui::rgb(0xffffff) } else { gpui::rgb(0x242429) })
        .text_color(if white { gpui::rgb(0x000000) } else { theme::text() })
        .hover(|s| s.opacity(0.9).shadow_md())
        .active(|s| s.opacity(0.65))
        .child(label.into())
}

impl AppView {
    fn toggle_row(
        &mut self,
        id: &'static str,
        title: &'static str,
        desc: &'static str,
        on: bool,
        f: impl Fn(&mut AppView, &mut Context<AppView>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = format!("srow-{id}");
        let h = super::interact::hover(&k);
        let el = row(id, title, Some(desc.into()), Some(switch(on, id)))
            .cursor_pointer()
            .bg(super::fx::with_alpha(gpui::rgb(0xffffff), 0.05 * h))
            .on_click(cx.listener(move |this, _, _, cx| f(this, cx)));
        super::interact::track(el, k, cx).into_any_element()
    }

    fn link_row(&mut self, id: &'static str, title: &'static str, url: &'static str, cx: &mut Context<Self>) -> AnyElement {
        row(id, title, None, Some(Icon::Forward.el(14., theme::text_secondary()).into_any_element()))
            .cursor_pointer()
            .hover(|s| s.bg(gpui::rgba(0xffffff0d)))
            .on_click(cx.listener(move |_, _, _, _| {
                let _ = open::that(url);
            }))
            .into_any_element()
    }

    pub fn settings_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tab = self.settings_tab;
        let tabs = div().flex().gap(sc(24.)).border_b_1().border_color(gpui::rgba(0xffffff12)).children(
            ["General", "Cuenta", "Acerca de"].into_iter().enumerate().map(|(i, t)| {
                let k = format!("stab-{i}");
                let h = super::interact::hover(&k);
                let el = div()
                    .id(("settings-tab", i))
                    .pb(sc(10.))
                    .text_size(sc(12.))
                    .font_weight(FontWeight::BOLD)
                    .cursor_pointer()
                    .text_color(if i == tab { theme::text() } else { super::interact::mix(theme::text_dim(), theme::text(), h) })
                    .border_b_2()
                    .border_color(if i == tab { theme::text() } else { super::fx::with_alpha(theme::text_dim(), h) })
                    .child(t)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_tab = i;
                        this.content_changed_pub();
                        cx.notify();
                    }));
                super::interact::track(el, k, cx)
            }),
        );
        let body = match tab {
            1 => self.settings_account(cx),
            2 => self.settings_about(cx),
            _ => self.settings_general(cx),
        };
        div()
            .max_w(sc(780.))
            .mx_auto()
            .flex()
            .flex_col()
            .child(div().pt(sc(26.)).pb(sc(16.)).text_size(sc(26.)).font_weight(FontWeight::BOLD).child("Ajustes"))
            .child(tabs)
            .child(body)
            .into_any_element()
    }

    fn settings_general(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();

        // ------------------------------------------------ audio quality
        // The radio dot sits before the text, like the official rows.
        let dot = |selected: bool| {
            div()
                .flex_none()
                .size(sc(18.))
                .rounded(sc(9.))
                .bg(gpui::rgb(0x1c1c20))
                .flex()
                .items_center()
                .justify_center()
                .when(selected, |d| d.bg(gpui::rgb(0x2a2a20)).child(div().size(sc(10.)).rounded(sc(5.)).bg(theme::gold())))
        };
        let quality_rows: Vec<AnyElement> = [
            (Quality::Low, "Equilibra calidad de audio y consumo de datos"),
            (Quality::High, "16-bit, 44,1 kHz"),
            (Quality::Max, "Hasta 24-bit, 192 kHz"),
        ]
        .into_iter()
        .map(|(q, sub)| {
            let selected = s.quality == q;
            div()
                .id(SharedString::from(format!("q-{}", q.label())))
                .px(sc(16.))
                .py(sc(14.))
                .flex()
                .items_center()
                .gap(sc(14.))
                .cursor_pointer()
                .when(selected, |d| {
                    d.bg(gpui::linear_gradient(
                        90.,
                        gpui::linear_color_stop(gpui::rgba(0xffd4322e), 0.),
                        gpui::linear_color_stop(gpui::rgba(0xffd43200), 0.7),
                    ))
                })
                .hover(|st| st.bg(gpui::rgba(0xffffff0d)))
                .child(dot(selected))
                .child(row_text(q.label(), Some(sub.into())))
                .when(q == Quality::Low, |d| d.child(small_button(("q-low-detail", 0usize), "AAC 320 kbps", false)))
                .on_click(cx.listener(move |this, _, _, cx| this.set_quality(q, cx)))
                .into_any_element()
        })
        .collect();

        // ------------------------------------------------ playback
        let device_label = s.output_device.clone().unwrap_or_else(|| "Predeterminado del sistema".into());
        let device_open = self.settings_dev_open;
        let mut playback = vec![
            self.toggle_row(
                "normalize",
                "Normalizar volumen",
                "Iguala el volumen de todas las pistas con los datos de ganancia de TIDAL.",
                s.normalize,
                |this, cx| this.toggle_setting("normalize", cx),
                cx,
            ),
            row(
                "sound-output",
                "Salida de sonido",
                None,
                Some(
                    small_button("sound-output-pick", device_label, false)
                        .max_w(sc(360.))
                        .child(Icon::ChevDown.el(12., theme::text_secondary()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings_dev_open = !this.settings_dev_open;
                            this.content_changed_pub();
                            cx.notify();
                        }))
                        .into_any_element(),
                ),
            )
            .into_any_element(),
        ];
        if device_open {
            let current = s.output_device.clone();
            let devices = self.devices.clone();
            playback.push(
                div()
                    .px(sc(8.))
                    .pb(sc(8.))
                    .flex()
                    .flex_col()
                    .children(std::iter::once(None).chain(devices.into_iter().map(Some)).enumerate().map(|(i, d)| {
                        let selected = d == current;
                        let label = d.clone().unwrap_or_else(|| "Predeterminado del sistema".into());
                        let dd = d.clone();
                        div()
                            .id(("dev", i))
                            .px(sc(10.))
                            .py(sc(8.))
                            .rounded(sc(6.))
                            .flex()
                            .items_center()
                            .gap(sc(10.))
                            .cursor_pointer()
                            .text_size(sc(12.))
                            .when(selected, |x| x.bg(gpui::rgb(0x242429)))
                            .hover(|x| x.bg(gpui::rgb(0x1c1c20)))
                            .child(Icon::VolumeUp.el(14., if selected { theme::accent() } else { theme::text_dim() }))
                            .child(div().truncate().child(label))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let s = this.settings.lock().clone();
                                this.settings_dev_open = false;
                                this.set_output(dd.clone(), s.exclusive_mode, s.force_volume, cx)
                            }))
                    }))
                    .into_any_element(),
            );
        }
        playback.push(self.toggle_row(
            "exclusive",
            "Modo exclusivo (WASAPI)",
            "Toma el control del dispositivo y envía el audio a su frecuencia original, sin pasar por el mezclador de Windows.",
            s.exclusive_mode,
            |this, cx| {
                let s = this.settings.lock().clone();
                this.set_output(s.output_device, !s.exclusive_mode, s.force_volume, cx)
            },
            cx,
        ));
        playback.push(self.toggle_row(
            "forcevol",
            "Forzar volumen",
            "En modo exclusivo fija el volumen al 100 % para una salida bit-perfect (controla el volumen en tu DAC o amplificador).",
            s.force_volume,
            |this, cx| {
                let s = this.settings.lock().clone();
                this.set_output(s.output_device, s.exclusive_mode, !s.force_volume, cx)
            },
            cx,
        ));
        playback.push(
            div()
                .px(sc(16.))
                .py(sc(10.))
                .text_size(sc(11.))
                .text_color(theme::text_dim())
                .child(if self.snap.output.is_empty() {
                    "Salida actual: se abrirá al reproducir".to_string()
                } else {
                    format!("Salida actual: {}", self.snap.output)
                })
                .into_any_element(),
        );
        let queue = vec![self.toggle_row(
            "autoplay",
            "Reproducción automática",
            "Sigue con canciones parecidas cuando termina la cola.",
            s.autoplay,
            |this, cx| this.toggle_setting("autoplay", cx),
            cx,
        )];

        // ------------------------------------------------ display
        let ambient = self.ambient(cx);
        let cover_url = self.now.as_ref().and_then(|n| n.track.cover(160));
        let swatch = |id: &'static str, label: &'static str, blur: bool, cx: &mut Context<Self>| {
            let selected = s.np_blur == blur;
            let url = cover_url.clone();
            let preview: AnyElement = if blur {
                match url.as_deref().and_then(|u| super::images::blurred(u, cx)) {
                    Some(img) => gpui::img(img).size_full().object_fit(gpui::ObjectFit::Cover).into_any_element(),
                    None => div().size_full().bg(gpui::rgb(0x6a4a55)).into_any_element(),
                }
            } else {
                div().size_full().bg(ambient).into_any_element()
            };
            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .gap(sc(6.))
                .cursor_pointer()
                .child(
                    div()
                        .w(sc(52.))
                        .h(sc(36.))
                        .rounded(sc(8.))
                        .overflow_hidden()
                        .border_2()
                        .border_color(if selected { theme::text() } else { gpui::rgba(0x00000000) })
                        .child(preview),
                )
                .child(
                    div()
                        .text_size(sc(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(if selected { theme::text() } else { theme::text_secondary() })
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings.lock().np_blur = blur;
                    this.settings.lock().save();
                    cx.notify();
                }))
                .into_any_element()
        };
        let player_bg = div()
            .px(sc(16.))
            .py(sc(14.))
            .flex()
            .items_center()
            .gap(sc(16.))
            .child(row_text("Fondo del reproductor", Some("Elige el aspecto del fondo de la pantalla de letras.".into())))
            .child(div().flex().gap(sc(12.)).child(swatch("bg-color", "Color", false, cx)).child(swatch("bg-blur", "Desenfoque", true, cx)))
            .into_any_element();
        let display1 = vec![
            player_bg,
            self.toggle_row("audio-meta", "Metadatos de audio", "Muestra campos adicionales (p. ej. BPM) en las listas de pistas.", s.audio_metadata, |this, cx| this.toggle_setting("audio_metadata", cx), cx),
            self.toggle_row("lyrics-ext", "Letras de otros proveedores", "Si TIDAL no tiene la letra, la busca en LRCLIB, NetEase y lyrics.ovh enviándoles el título, artista y álbum de la canción.", s.external_lyrics, |this, cx| this.toggle_setting("external_lyrics", cx), cx),
            self.toggle_row("lyrics-roman", "Pronunciación de letras", "Muestra la lectura en alfabeto latino bajo las letras en japonés (romaji), chino (pinyin) y coreano.", s.lyrics_romanize, |this, cx| this.toggle_setting("lyrics_romanize", cx), cx),
            self.toggle_row("show-ratings", "Valoraciones", "Muestra la columna de estrellas en las listas de pistas.", s.show_ratings, |this, cx| this.toggle_setting("show_ratings", cx), cx),
        ];
        let display2 = vec![self.toggle_row(
            "hide-uploads",
            "Ocultar la pestaña Uploads",
            "Quita la pestaña Uploads del Inicio.",
            s.hide_uploads,
            |this, cx| this.toggle_setting("hide_uploads", cx),
            cx,
        )];

        // ------------------------------------------------ connect
        let connect = vec![self.toggle_row(
            "discord",
            "Discord",
            "Muestra «Escuchando TIDE» en tu perfil de Discord (visible para tus contactos y servidores): canción, artista, álbum, portada y progreso.",
            s.discord,
            |this, cx| this.toggle_setting("discord", cx),
            cx,
        )];

        // ------------------------------------------------ preferences
        let zoom_pct = format!("{} %", (super::zoom() * 100.0).round() as i32);
        let zoom_row = row(
            "zoom-row",
            "Zoom de la interfaz",
            Some("También con Ctrl + / Ctrl − / Ctrl 0 o Ctrl + rueda.".into()),
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(sc(8.))
                    .child(small_button("zoom-out", "−", false).on_click(cx.listener(|this, _, _, cx| this.step_zoom(-1, cx))))
                    .child(div().w(sc(46.)).text_center().text_size(sc(11.)).font_weight(FontWeight::BOLD).child(zoom_pct))
                    .child(small_button("zoom-in", "+", false).on_click(cx.listener(|this, _, _, cx| this.step_zoom(1, cx))))
                    .into_any_element(),
            ),
        )
        .into_any_element();
        let prefs = vec![zoom_row];
        let legal = vec![
            self.link_row("terms", "Términos y condiciones", "https://tidal.com/terms", cx),
            self.link_row("privacy", "Aviso de privacidad", "https://tidal.com/privacy", cx),
        ];

        div()
            .flex()
            .flex_col()
            .child(heading("Calidad de audio"))
            .child(card(quality_rows))
            .child(heading("Reproducción"))
            .child(card(playback))
            .child(div().h(sc(12.)))
            .child(card(queue))
            .child(heading("Pantalla"))
            .child(card(display1))
            .child(div().h(sc(12.)))
            .child(card(display2))
            .child(heading("Conectar"))
            .child(card(connect))
            .child(heading("Preferencias"))
            .child(card(prefs))
            .child(div().h(sc(12.)))
            .child(card(legal))
            .into_any_element()
    }

    fn settings_account(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.lock().clone();
        let name = s.username.clone().unwrap_or_default();
        let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
        let avatar = div()
            .flex_none()
            .size(sc(44.))
            .rounded(sc(22.))
            .bg(gpui::rgb(0x303036))
            .flex()
            .items_center()
            .justify_center()
            .text_size(sc(16.))
            .font_weight(FontWeight::BOLD)
            .child(initial);
        let profile_card = div()
            .px(sc(16.))
            .py(sc(14.))
            .flex()
            .items_center()
            .gap(sc(14.))
            .child(avatar)
            .child(div().flex_1().text_size(sc(13.)).font_weight(FontWeight::BOLD).child(name.clone()))
            .child(small_button("edit-profile", "Editar perfil", false).on_click(cx.listener(|_, _, _, _| {
                let _ = open::that("https://tidal.com/profile");
            })))
            .into_any_element();
        let manage = |id: &'static str| {
            small_button(id, "Gestionar", false).on_click(|_, _, _| {
                let _ = open::that("https://account.tidal.com");
            })
        };
        let personal = vec![
            row("first-name", "Nombre", None, Some(manage("manage-name").into_any_element())).into_any_element(),
            row("email", "Correo electrónico", Some(name.clone().into()), None).into_any_element(),
        ];
        let account = vec![
            row("sub", "Suscripción actual", Some("TIDAL".into()), Some(manage("manage-sub").into_any_element())).into_any_element(),
            row("username", "Nombre de usuario", Some(name.into()), None).into_any_element(),
        ];
        let more = vec![
            self.link_row("notif", "Ajustes de notificaciones", "https://account.tidal.com", cx),
            self.link_row("pass", "Cambiar contraseña", "https://account.tidal.com", cx),
            row(
                "signout",
                "Cerrar sesión",
                Some("Borra la sesión guardada en este equipo.".into()),
                Some(
                    small_button("signout-btn", "Cerrar sesión", true)
                        .on_click(cx.listener(|this, _, _, cx| this.sign_out(cx)))
                        .into_any_element(),
                ),
            )
            .into_any_element(),
        ];
        div()
            .flex()
            .flex_col()
            .pt(sc(20.))
            .child(card(vec![profile_card]))
            .child(heading("Datos personales"))
            .child(card(personal))
            .child(heading("Cuenta"))
            .child(card(account))
            .child(div().h(sc(12.)))
            .child(card(more))
            .into_any_element()
    }

    fn settings_about(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let header = div()
            .px(sc(16.))
            .py(sc(16.))
            .flex()
            .items_center()
            .gap(sc(16.))
            .child(gpui::img(super::images::logo()).size(sc(56.)).rounded(sc(12.)))
            .child(row_text(
                "Versión",
                Some(format!("TIDE {} · cliente no oficial de TIDAL, hecho con Rust y GPUI", env!("CARGO_PKG_VERSION")).into()),
            ))
            .into_any_element();
        const CREDITS: &[(&str, &str)] = &[
            ("GPUI", "Apache-2.0: interfaz"),
            ("symphonia", "MPL-2.0: decodificación FLAC y AAC"),
            ("rubato", "MIT: remuestreo sinc"),
            ("cpal / wasapi", "Apache-2.0 / MIT: salida de audio"),
            ("reqwest, serde, image, roxmltree", "MIT / Apache-2.0"),
            ("lindera + IPADIC", "MIT / licencia IPADIC: lectura de kanji (romaji)"),
            ("pinyin", "MIT: pinyin del chino"),
            ("Inter", "SIL OFL 1.1: tipografía"),
            ("Material Icons", "Apache-2.0: iconos"),
        ];
        let credits = div()
            .px(sc(16.))
            .py(sc(14.))
            .flex()
            .flex_col()
            .gap(sc(8.))
            .children(CREDITS.iter().map(|(n, l)| {
                div()
                    .flex()
                    .gap(sc(10.))
                    .text_size(sc(11.))
                    .child(div().w(sc(210.)).flex_none().font_weight(FontWeight::BOLD).child(*n))
                    .child(div().text_color(theme::text_secondary()).child(*l))
            }))
            .into_any_element();
        div()
            .flex()
            .flex_col()
            .pt(sc(20.))
            .child(card(vec![header]))
            .child(heading("TIDE está construido con"))
            .child(card(vec![credits]))
            .child(
                div()
                    .pt(sc(18.))
                    .px(sc(12.))
                    .text_size(sc(11.))
                    .text_color(theme::text_dim())
                    .child("TIDE es un proyecto independiente: no está afiliado a TIDAL ni cuenta con su respaldo. Necesitas tu propia suscripción a TIDAL."),
            )
            .into_any_element()
    }
}

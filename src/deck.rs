//! The compact player: Winamp's way of working (main panel, equalizer, playlist stacked in one
//! small window) drawn in ytmfast's own colours. `.wsz` skins stay available in winamp.rs.

use eframe::egui::{self, Color32, Rect, Sense, pos2, vec2};

use crate::settings::{DeckStyle, Repeat, Settings};
use crate::theme::c;
use crate::ui::{self, Icon};
use crate::{Act, App};
use crate::i18n::tr;

pub const W: f32 = 320.0;
const MAIN_H: f32 = 132.0;
const EQ_H: f32 = 150.0;
const PL_H: f32 = 228.0;
const STRIP: f32 = 18.0;
const ROW: f32 = 18.0;

pub fn height(s: &Settings) -> f32 {
    MAIN_H + if s.deck_eq { EQ_H } else { 0.0 } + if s.deck_playlist { PL_H } else { 0.0 }
}

/// Light and shadow of the panel colour, for the classic style's bevels.
fn shades() -> (Color32, Color32) {
    let p = c();
    (p.bar.lerp_to_gamma(Color32::WHITE, 0.4), p.bar.lerp_to_gamma(Color32::BLACK, 0.65))
}

/// A one-pixel bevel around `r`: light top-left and dark bottom-right when raised, the other way
/// round when sunken.
fn bevel(ui: &egui::Ui, r: Rect, raised: bool) {
    let (hi, lo) = shades();
    let (tl, br) = if raised { (hi, lo) } else { (lo, hi) };
    let painter = ui.painter();
    painter.line_segment([r.left_bottom(), r.left_top()], egui::Stroke::new(1.0, tl));
    painter.line_segment([r.left_top(), r.right_top()], egui::Stroke::new(1.0, tl));
    painter.line_segment([r.right_top(), r.right_bottom()], egui::Stroke::new(1.0, br));
    painter.line_segment([r.right_bottom(), r.left_bottom()], egui::Stroke::new(1.0, br));
}

/// In the classic style, a control sits in a raised frame that sinks while held.
fn framed(resp: egui::Response, ui: &egui::Ui, style: DeckStyle) -> egui::Response {
    if style == DeckStyle::Classic {
        bevel(ui, resp.rect.shrink(1.0), !resp.is_pointer_button_down_on());
    }
    resp
}

/// Title strip: name centred between thin double rules, like Winamp's window bars (engraved
/// grooves in the classic style).
fn strip(ui: &egui::Ui, r: Rect, title: &str, style: DeckStyle) {
    let p = c();
    let painter = ui.painter();
    let g = painter.layout_no_wrap(title.to_string(), ui::bold(9.5), p.text2);
    let tw = g.size().x + 16.0;
    let cy = r.center().y;
    let (hi, lo) = shades();
    for dy in [-2.0, 2.0] {
        for (a, b) in [(r.left() + 8.0, r.center().x - tw / 2.0), (r.center().x + tw / 2.0, r.right() - 44.0)] {
            if b > a {
                if style == DeckStyle::Classic {
                    painter.line_segment([pos2(a, cy + dy - 0.5), pos2(b, cy + dy - 0.5)], egui::Stroke::new(1.0, lo));
                    painter.line_segment([pos2(a, cy + dy + 0.5), pos2(b, cy + dy + 0.5)], egui::Stroke::new(1.0, hi));
                } else {
                    painter.line_segment([pos2(a, cy + dy), pos2(b, cy + dy)], egui::Stroke::new(1.0, p.line));
                }
            }
        }
    }
    painter.galley(pos2(r.center().x - g.size().x / 2.0, cy - g.size().y / 2.0), g, p.text2);
}

/// Small text toggle ("EQ", "PL", "ON").
fn tag(ui: &mut egui::Ui, label: &str, on: bool, hint: &str) -> egui::Response {
    let p = c();
    let w = ui.painter().layout_no_wrap(label.to_string(), ui::bold(9.0), p.text).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2((w + 10.0).max(26.0), 16.0), Sense::click());
    let fill = if on { p.accent } else if resp.hovered() { p.hover } else { Color32::TRANSPARENT };
    ui.painter().rect(r, 3.0, fill, egui::Stroke::new(1.0, if on { p.accent } else { p.line }), egui::StrokeKind::Inside);
    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, label, ui::bold(9.0), if on { Color32::WHITE } else { p.text2 });
    resp.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Section background: the bar colour with a hairline frame (raised chrome in the classic style).
fn panel(ui: &egui::Ui, r: Rect, style: DeckStyle) {
    let p = c();
    if style == DeckStyle::Classic {
        let (hi, lo) = shades();
        ui::gradient(ui, r, p.bar.lerp_to_gamma(hi, 0.35), p.bar.lerp_to_gamma(lo, 0.25));
        bevel(ui, r.shrink(0.5), true);
        bevel(ui, r.shrink(1.5), true);
        return;
    }
    ui.painter().rect(r.shrink(0.5), 0.0, p.bar, egui::Stroke::new(1.0, p.line), egui::StrokeKind::Inside);
}

/// Recessed area (display, playlist): square and sunken in the classic style.
fn inset(ui: &egui::Ui, r: Rect, style: DeckStyle) {
    let p = c();
    if style == DeckStyle::Classic {
        ui.painter().rect_filled(r, 0.0, p.bg.lerp_to_gamma(Color32::BLACK, if p.dark { 0.35 } else { 0.0 }));
        bevel(ui, r.shrink(0.5), false);
        return;
    }
    ui.painter().rect(r, 5.0, p.bg, egui::Stroke::new(1.0, p.line), egui::StrokeKind::Inside);
}

/// The LCD style's palette: a pale screen tinted by the theme's accent, everything inked in one
/// dark colour.
fn lcd(p: crate::theme::Palette) -> crate::theme::Palette {
    let screen = Color32::from_gray(226).lerp_to_gamma(p.accent, 0.22);
    let ink = Color32::from_gray(24).lerp_to_gamma(p.accent, 0.25);
    crate::theme::Palette {
        dark: false,
        bg: screen.lerp_to_gamma(ink, 0.06),
        bar: screen,
        hover: ink.gamma_multiply(0.12),
        selected: ink.gamma_multiply(0.2),
        line: ink.gamma_multiply(0.45),
        text: ink,
        text2: ink.gamma_multiply(0.7),
        accent: ink,
    }
}

/// Segments lit for each character, bits a..g (top, top-right, bottom-right, bottom, bottom-left,
/// top-left, middle).
fn segments(ch: char) -> u8 {
    match ch {
        '0' => 0b0111111,
        '1' => 0b0000110,
        '2' => 0b1011011,
        '3' => 0b1001111,
        '4' => 0b1100110,
        '5' => 0b1101101,
        '6' => 0b1111101,
        '7' => 0b0000111,
        '8' => 0b1111111,
        '9' => 0b1101111,
        '-' => 0b1000000,
        _ => 0,
    }
}

/// Text in seven-segment digits, right-aligned at `right` (vertically centred on it), with the
/// unlit segments faintly visible as on a real display.
fn seven_segment(ui: &egui::Ui, right: egui::Pos2, text: &str, h: f32, on: Color32) {
    let off = on.gamma_multiply(0.1);
    let (w, t) = (h * 0.5, (h * 0.13).max(2.0));
    let painter = ui.painter();
    let mut x = right.x;
    let top = right.y - h / 2.0;
    for ch in text.chars().rev() {
        if ch == ':' {
            x -= t * 2.5;
            for y in [top + h * 0.3, top + h * 0.7] {
                painter.rect_filled(Rect::from_center_size(pos2(x + t * 1.25, y), vec2(t, t)), 0.0, on);
            }
            continue;
        }
        x -= w + t * 1.6;
        let bits = segments(ch);
        let (l, r, m, b) = (x, x + w, top + h / 2.0, top + h);
        // (bit, rect) for each segment, slightly short of the corners like real ones.
        let g = t * 0.6;
        let segs = [
            (0, Rect::from_min_max(pos2(l + g, top), pos2(r - g, top + t))),
            (1, Rect::from_min_max(pos2(r - t, top + g), pos2(r, m - g / 2.0))),
            (2, Rect::from_min_max(pos2(r - t, m + g / 2.0), pos2(r, b - g))),
            (3, Rect::from_min_max(pos2(l + g, b - t), pos2(r - g, b))),
            (4, Rect::from_min_max(pos2(l, m + g / 2.0), pos2(l + t, b - g))),
            (5, Rect::from_min_max(pos2(l, top + g), pos2(l + t, m - g / 2.0))),
            (6, Rect::from_min_max(pos2(l + g, m - t / 2.0), pos2(r - g, m + t / 2.0))),
        ];
        for (bit, rect) in segs {
            painter.rect_filled(rect, t * 0.3, if bits >> bit & 1 == 1 { on } else { off });
        }
    }
}

fn small_button(ui: &mut egui::Ui, r: Rect, id: &str, icon: Icon, hint: &str) -> bool {
    let resp = ui.interact(r, ui.id().with(id), Sense::click());
    let p = c();
    if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, p.hover);
    }
    ui::paint_icon(ui.painter(), icon, r.shrink(3.0), if resp.hovered() { p.text } else { p.text2 });
    resp.on_hover_text(hint).clicked()
}

impl App {
    pub(crate) fn deck(&mut self, ui: &mut egui::Ui) {
        if self.s.deck_style == DeckStyle::Lcd {
            return crate::theme::with_palette(lcd(c()), || self.deck_sections(ui));
        }
        self.deck_sections(ui);
    }

    fn deck_sections(&mut self, ui: &mut egui::Ui) {
        self.pull_tap();
        // Frames without new audio keep the bars; silence lets them fall.
        if !self.pcm.is_empty() || self.pl.paused || self.pl.pos < 0 {
            crate::winamp::analyse(&self.pcm, &mut self.bars);
        }
        if self.pl.pos >= 0 && !self.pl.paused || self.bars.iter().any(|b| *b > 0.0) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        // The window is locked to this size; drawing at it also keeps a compositor that ignored the
        // resize from stretching the layout.
        let full = Rect::from_min_size(ui.max_rect().min, vec2(W, height(&self.s)).min(ui.max_rect().size()));
        // Like Winamp, any empty spot moves the window (a compositor may place it with the title strip
        // off-screen); widgets added afterwards take precedence over this.
        let bg = ui.interact(full, ui.id().with("deck-bg"), Sense::drag());
        ui::drag_window(ui, &bg);
        let main = Rect::from_min_size(full.min, vec2(full.width(), MAIN_H));
        self.deck_main(ui, main);
        let mut y = main.bottom();
        if self.s.deck_eq {
            let r = Rect::from_min_size(pos2(full.left(), y), vec2(full.width(), EQ_H));
            self.deck_eq(ui, r);
            y = r.bottom();
        }
        if self.s.deck_playlist {
            self.deck_playlist(ui, Rect::from_min_max(pos2(full.left(), y), full.right_bottom()));
        }
    }

    fn deck_main(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = c();
        let style = self.s.deck_style;
        panel(ui, r, style);
        // Title strip: drag to move; minimise and back-to-app on the right.
        let head = Rect::from_min_size(r.min, vec2(r.width(), STRIP + 2.0));
        ui::paint_logo(ui.painter(), Rect::from_min_size(head.min + vec2(6.0, 4.0), vec2(12.0, 12.0)));
        strip(ui, head, "YTMFAST", style);
        let b = |i: f32| Rect::from_min_size(pos2(head.right() - 20.0 * i, head.top() + 3.0), vec2(15.0, 15.0));
        if small_button(ui, b(1.0), "deck-full", Icon::Expand, tr("Voltar ao app (Ctrl+M)")) {
            self.acts.push(Act::Mini(false));
        }
        if small_button(ui, b(2.0), "deck-min", Icon::Minimize, tr("Minimizar")) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        // Display: state + time, analyser, scrolling title, stream info.
        let d = Rect::from_min_max(pos2(r.left() + 8.0, head.bottom() + 2.0), pos2(r.right() - 8.0, head.bottom() + 58.0));
        inset(ui, d, style);
        let playing = self.pl.pos >= 0;
        let glyph = Rect::from_min_size(d.min + vec2(9.0, 11.0), vec2(9.0, 9.0));
        match (playing, self.pl.paused) {
            (true, false) => drop(ui.painter().add(egui::Shape::convex_polygon(vec![glyph.left_top(), glyph.right_center(), glyph.left_bottom()], p.accent, egui::Stroke::NONE))),
            (true, true) => {
                ui.painter().rect_filled(Rect::from_min_size(glyph.min, vec2(3.0, 9.0)), 0.0, p.accent);
                ui.painter().rect_filled(Rect::from_min_size(glyph.min + vec2(6.0, 0.0), vec2(3.0, 9.0)), 0.0, p.accent);
            }
            _ => drop(ui.painter().rect_filled(glyph.shrink(0.5), 0.0, p.text2)),
        }
        // Click the time to switch between elapsed and remaining, as in Winamp.
        let time_r = Rect::from_min_size(d.min + vec2(22.0, 3.0), vec2(76.0, 26.0));
        let rem_id = ui.id().with("deck-remaining");
        let mut remaining: bool = ui.data(|m| m.get_temp(rem_id)).unwrap_or(false);
        if ui.interact(time_r, ui.id().with("deck-time"), Sense::click()).on_hover_text(tr("Tempo decorrido / restante")).clicked() {
            remaining = !remaining;
            ui.data_mut(|m| m.insert_temp(rem_id, remaining));
        }
        if playing {
            let t = if remaining { format!("-{}", ui::fmt_time((self.pl.dur - self.pl.time).max(0.0))) } else { ui::fmt_time(self.pl.time) };
            if style == DeckStyle::Modern {
                ui.painter().text(time_r.right_center(), egui::Align2::RIGHT_CENTER, t, egui::FontId::monospace(24.0), p.accent);
            } else {
                seven_segment(ui, time_r.right_center(), &t, 20.0, p.accent);
            }
        } else if style != DeckStyle::Modern {
            // Unlit digits, as an idle display shows.
            seven_segment(ui, time_r.right_center(), "    ", 20.0, p.accent);
        }
        // Analyser: 19 segmented bars.
        let ax = d.left() + 9.0;
        let ab = d.bottom() - 6.0;
        for (i, b) in self.bars.iter().enumerate() {
            let segs = (b.clamp(0.0, 1.0) * 8.0).round() as i32;
            for s in 0..segs {
                let col = p.accent.gamma_multiply(0.35 + 0.65 * s as f32 / 7.0);
                ui.painter().rect_filled(Rect::from_min_size(pos2(ax + i as f32 * 4.5, ab - 2.0 - s as f32 * 2.5), vec2(3.5, 2.0)), 0.0, col);
            }
        }
        // Scrolling title.
        let tx = d.left() + 104.0;
        let tw = d.right() - 8.0 - tx;
        let title = match self.current() {
            Some(t) => format!("{}. {} - {} ({})", self.pl.pos + 1, ui::artists(t), t.name, ui::fmt_time(self.pl.dur)),
            None => tr("YtmFast — nada tocando").into(),
        };
        let g = ui.painter().layout_no_wrap(title.clone(), ui::medium(12.0), p.text);
        let clip = Rect::from_min_size(pos2(tx, d.top() + 6.0), vec2(tw, 18.0));
        let painter = ui.painter().with_clip_rect(clip);
        if g.size().x <= tw {
            painter.galley(clip.left_top(), g, p.text);
        } else {
            let gap = 40.0;
            let span = g.size().x + gap;
            let off = (ui.input(|i| i.time) as f32 * 28.0) % span;
            painter.galley(clip.left_top() - vec2(off, 0.0), g.clone(), p.text);
            painter.galley(clip.left_top() + vec2(span - off, 0.0), g, p.text);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        // Stream info, or the line being sung when the lyrics are timed.
        let m = self.mpv;
        self.load_lyrics();
        if let Some(line) = self.lyric_now().filter(|_| playing) {
            let g = ui::line(ui, &format!("♪ {line}"), ui::medium(11.0), p.accent, d.right() - 8.0 - tx);
            ui.painter().galley(pos2(tx, d.top() + 30.0), g, p.accent);
        } else if playing {
            let kbps = (m.get_property::<f64>("audio-bitrate").unwrap_or(0.0) / 1000.0).round();
            let khz = m.get_property::<i64>("audio-params/samplerate").unwrap_or(0) as f64 / 1000.0;
            let info = format!("{kbps:.0} kbps   {khz:.1} kHz");
            ui.painter().text(pos2(tx, d.top() + 30.0), egui::Align2::LEFT_TOP, info, ui::regular(10.5), p.text2);
            let stereo = m.get_property::<i64>("audio-params/channel-count").unwrap_or(2) >= 2;
            let badge = |x: f32, label: &str, on: bool| {
                ui.painter().text(pos2(x, d.top() + 31.0), egui::Align2::RIGHT_TOP, label, ui::bold(9.0), if on { p.accent } else { p.text2.gamma_multiply(0.45) });
            };
            badge(d.right() - 8.0, tr("ESTÉREO"), stereo);
            badge(d.right() - 58.0, "MONO", !stereo);
        }

        // Seek.
        let seek = Rect::from_min_size(pos2(r.left() + 8.0, d.bottom() + 4.0), vec2(r.width() - 16.0, 14.0));
        let frac = if self.pl.dur > 0.0 { (self.pl.time / self.pl.dur) as f32 } else { 0.0 };
        let (resp, f) = ui::seek_bar(ui, seek, "deck-seek", frac, p.accent);
        if (resp.drag_stopped() || resp.clicked()) && self.pl.dur > 0.0 {
            m.command("seek", &[&(f as f64 * self.pl.dur).to_string(), "absolute"]).ok();
        }

        // Controls.
        let row = Rect::from_min_max(pos2(r.left() + 6.0, seek.bottom() + 2.0), pos2(r.right() - 8.0, r.bottom() - 6.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let paused = self.pl.paused || !playing;
            if framed(ui::icon_button(ui, Icon::Prev, 26.0), ui, style).on_hover_text(tr("Anterior")).clicked() {
                m.command("playlist-prev", &[]).ok();
            }
            if framed(ui::icon_toggle(ui, Icon::Play, 26.0, playing && !paused), ui, style).on_hover_text(tr("Tocar")).clicked() {
                if !playing && !self.queue.is_empty() {
                    m.command("playlist-play-index", &["0"]).ok();
                }
                m.set_property("pause", false).ok();
            }
            if framed(ui::icon_toggle(ui, Icon::Pause, 26.0, playing && paused), ui, style).on_hover_text(tr("Pausar")).clicked() {
                m.command("cycle", &["pause"]).ok();
            }
            if framed(ui::icon_button(ui, Icon::Stop, 26.0), ui, style).on_hover_text(tr("Parar")).clicked() {
                m.set_property("pause", true).ok();
                m.command("seek", &["0", "absolute"]).ok();
            }
            if framed(ui::icon_button(ui, Icon::Next, 26.0), ui, style).on_hover_text(tr("Próxima")).clicked() {
                m.command("playlist-next", &[]).ok();
            }
            ui.add_space(8.0);
            // Volume: drag or mouse wheel.
            let (vr, _) = ui.allocate_exact_size(vec2(58.0, 18.0), Sense::hover());
            let muted: bool = m.get_property("mute").unwrap_or(false);
            let (vresp, v) = ui::seek_bar(ui, vr, "deck-vol", if muted { 0.0 } else { (self.pl.vol / 100.0) as f32 }, p.text);
            if vresp.is_pointer_button_down_on() {
                m.set_property("mute", false).ok();
                m.set_property("volume", v as f64 * 100.0).ok();
            }
            if vresp.drag_stopped() || vresp.clicked() {
                self.s.volume = v as f64 * 100.0;
            }
            let scroll = if vresp.hovered() { ui.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
            if scroll != 0.0 {
                let nv = (self.pl.vol + scroll as f64 * 0.1).clamp(0.0, 100.0);
                m.set_property("volume", nv).ok();
                self.s.volume = nv;
            }
            vresp.on_hover_text(tf!("Volume {}%", format!("{:.0}", self.pl.vol)));
            ui.add_space(4.0);
            if framed(ui::icon_toggle(ui, Icon::Shuffle, 24.0, self.s.shuffle), ui, style).on_hover_text(tr("Aleatório")).clicked() {
                self.acts.push(Act::Shuffle);
            }
            let rep = if self.s.repeat == Repeat::One { Icon::RepeatOne } else { Icon::Repeat };
            if framed(ui::icon_toggle(ui, rep, 24.0, self.s.repeat != Repeat::Off), ui, style).on_hover_text(tr("Repetir")).clicked() {
                self.acts.push(Act::Repeat);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if tag(ui, "PL", self.s.deck_playlist, tr("Fila")).clicked() {
                    self.s.deck_playlist = !self.s.deck_playlist;
                }
                if tag(ui, "EQ", self.s.deck_eq, tr("Equalizador")).clicked() {
                    self.s.deck_eq = !self.s.deck_eq;
                }
            });
        });
    }

    fn deck_eq(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = c();
        let style = self.s.deck_style;
        panel(ui, r, style);
        let head = Rect::from_min_size(r.min, vec2(r.width(), STRIP + 2.0));
        strip(ui, head, tr("EQUALIZADOR"), style);
        if small_button(ui, Rect::from_min_size(pos2(head.right() - 20.0, head.top() + 3.0), vec2(15.0, 15.0)), "deck-eq-x", Icon::Close, tr("Fechar")) {
            self.s.deck_eq = false;
        }
        // On/off and presets.
        let bar = Rect::from_min_max(pos2(r.left() + 8.0, head.bottom()), pos2(r.right() - 8.0, head.bottom() + 22.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if tag(ui, "ON", self.s.eq_on, tr("Ligar / desligar o equalizador")).clicked() {
                self.s.eq_on = !self.s.eq_on;
            }
            if tag(ui, "AUTO", self.s.eq_loudness, tr("Loudness adaptativo: mais graves e agudos com volume baixo")).clicked() {
                self.s.eq_loudness = !self.s.eq_loudness;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current = crate::views::EQ_PRESETS.iter().find(|(_, b)| *b == self.s.eq_bands_db).map_or(tr("Personalizado"), |(n, _)| tr(n));
                egui::ComboBox::from_id_salt("deck-preset").selected_text(egui::RichText::new(current).size(11.0)).width(130.0).show_ui(ui, |ui| {
                    for (name, bands) in crate::views::EQ_PRESETS {
                        if ui.selectable_label(self.s.eq_bands_db == *bands, tr(name)).clicked() {
                            self.s.eq_bands_db = *bands;
                            self.s.eq_on = true;
                        }
                    }
                });
            });
        });
        // Preamp + 10 bands as vertical faders.
        let area = Rect::from_min_max(pos2(r.left() + 10.0, bar.bottom() + 6.0), pos2(r.right() - 10.0, r.bottom() - 18.0));
        let names = [tr("Pré"), "60", "170", "310", "600", "1k", "3k", "6k", "12k", "14k", "16k"];
        let col_w = area.width() / 11.0;
        let y_of = |db: f32| area.top() + (12.0 - db) / 24.0 * area.height();
        ui.painter().line_segment([pos2(area.left(), y_of(0.0)), pos2(area.right(), y_of(0.0))], egui::Stroke::new(1.0, p.line));
        let dim = if self.s.eq_on { 1.0 } else { 0.4 };
        for (i, name) in names.iter().enumerate() {
            // A little gap after the preamp, as in Winamp.
            let cx = area.left() + col_w * (i as f32 + 0.5) + if i == 0 { -4.0 } else { 2.0 };
            let col = Rect::from_center_size(pos2(cx, area.center().y), vec2(col_w - 4.0, area.height()));
            let resp = ui.interact(col, ui.id().with(("deck-band", i)), Sense::click_and_drag());
            let val = if i == 0 { &mut self.s.eq_preamp_db } else { &mut self.s.eq_bands_db[i - 1] };
            if let Some(pp) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
                let db = 12.0 - (pp.y - area.top()) / area.height() * 24.0;
                // Snaps to 0 dB near the middle.
                *val = if db.abs() < 0.6 { 0.0 } else { db.clamp(-12.0, 12.0) };
            }
            if resp.double_clicked() {
                *val = 0.0;
            }
            let y = y_of(*val);
            ui.painter().line_segment([pos2(cx, area.top()), pos2(cx, area.bottom())], egui::Stroke::new(2.0, p.line));
            let fill_top = y.min(y_of(0.0));
            let fill_bot = y.max(y_of(0.0));
            ui.painter().rect_filled(Rect::from_min_max(pos2(cx - 1.0, fill_top), pos2(cx + 1.0, fill_bot)), 0.0, p.accent.gamma_multiply(dim));
            let knob = Rect::from_center_size(pos2(cx, y), vec2(12.0, 6.0));
            ui.painter().rect_filled(knob, 2.0, if resp.hovered() || resp.dragged() { p.text } else { p.text.gamma_multiply(0.8 * dim) });
            ui.painter().text(pos2(cx, area.bottom() + 4.0), egui::Align2::CENTER_TOP, *name, ui::regular(9.0), p.text2);
            resp.on_hover_text(tf!("{}: {} dB · duplo clique zera", name, format!("{:+.1}", *if i == 0 { &self.s.eq_preamp_db } else { &self.s.eq_bands_db[i - 1] })));
        }
    }

    fn deck_playlist(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = c();
        let style = self.s.deck_style;
        panel(ui, r, style);
        let head = Rect::from_min_size(r.min, vec2(r.width(), STRIP + 2.0));
        strip(ui, head, tr("FILA"), style);
        if small_button(ui, Rect::from_min_size(pos2(head.right() - 20.0, head.top() + 3.0), vec2(15.0, 15.0)), "deck-pl-x", Icon::Close, tr("Fechar")) {
            self.s.deck_playlist = false;
        }
        let list = Rect::from_min_max(pos2(r.left() + 6.0, head.bottom()), pos2(r.right() - 6.0, r.bottom() - 24.0));
        inset(ui, list, style);
        let cur = usize::try_from(self.pl.pos).ok();
        // Follow the playing track when it changes.
        let follow_id = ui.id().with("deck-follow");
        let last: Option<usize> = ui.data(|m| m.get_temp(follow_id)).flatten();
        let follow = last != cur;
        ui.data_mut(|m| m.insert_temp(follow_id, cur));
        let mut acts = vec![];
        ui.scope_builder(egui::UiBuilder::new().max_rect(list.shrink2(vec2(2.0, 3.0))), |ui| {
            if self.queue.is_empty() {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(tr("  A fila está vazia.")).color(p.text2).size(11.5));
                return;
            }
            egui::ScrollArea::vertical().id_salt("deck-pl").auto_shrink(false).show_rows(ui, ROW, self.queue.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for i in range {
                    let t = &self.queue[i];
                    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    let on = cur == Some(i);
                    if resp.hovered() {
                        ui.painter().rect_filled(row, 2.0, p.hover);
                    }
                    let col = if on { p.accent } else { p.text };
                    let font = if on { ui::medium(11.5) } else { ui::regular(11.5) };
                    let dur = t.duration.map(|d| ui::fmt_time(d as f64)).unwrap_or_default();
                    let dur_g = ui.painter().layout_no_wrap(dur, egui::FontId::monospace(10.5), if on { p.accent } else { p.text2 });
                    let text = format!("{}. {} - {}", i + 1, ui::artists(t), t.name);
                    let g = ui::line(ui, &text, font, col, row.width() - dur_g.size().x - 16.0);
                    ui.painter().galley(pos2(row.left() + 4.0, row.center().y - g.size().y / 2.0), g, col);
                    ui.painter().galley(pos2(row.right() - 4.0 - dur_g.size().x, row.center().y - dur_g.size().y / 2.0), dur_g, col);
                    if on && follow {
                        ui.scroll_to_rect(row, Some(egui::Align::Center));
                    }
                    if resp.double_clicked() {
                        acts.push(Act::Jump(i));
                    }
                    resp.on_hover_text(tr("Duplo clique toca")).context_menu(|ui| {
                        if ui.button(tr("Tocar agora")).clicked() {
                            acts.push(Act::Jump(i));
                            ui.close();
                        }
                        if !on && ui.button(tr("Remover da fila")).clicked() {
                            acts.push(Act::Unqueue(i));
                            ui.close();
                        }
                    });
                }
            });
        });
        self.acts.extend(acts);
        // Footer: count and time, like Winamp's running time.
        let foot = Rect::from_min_max(pos2(r.left() + 10.0, list.bottom()), pos2(r.right() - 10.0, r.bottom()));
        let total: f64 = self.queue.iter().filter_map(|t| t.duration).map(f64::from).sum();
        let n = self.queue.len();
        ui.painter().text(foot.left_center(), egui::Align2::LEFT_CENTER, if n == 1 { tf!("{} faixa", n) } else { tf!("{} faixas", n) }, ui::regular(10.5), p.text2);
        let shown = if cur.is_some() { ui::fmt_time(self.pl.time) } else { "0:00".into() };
        ui.painter().text(foot.right_center(), egui::Align2::RIGHT_CENTER, format!("{shown} / {}", ui::fmt_time(total)), egui::FontId::monospace(10.5), p.text2);
    }
}

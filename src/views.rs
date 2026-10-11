//! Every screen of the app. Views only draw and push `Act`s; `App::apply` does the work.

use eframe::egui::{self, Color32, Rect, RichText, pos2, vec2};
use rustypipe::model::TrackItem;

use crate::settings::{Quality, Repeat, cache_dir, config_dir, state_dir};
use crate::theme::{self, c};
use crate::ui::{self, Icon, RowClick, RowCtx};
use crate::{Act, App, ListKind, View};
use crate::i18n::tr;

fn album_sub(a: &rustypipe::model::AlbumItem) -> String {
    let artists = a.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
    match a.year {
        Some(y) => format!("{artists} · {y}"),
        None => artists,
    }
}

fn playlist_sub(p: &rustypipe::model::MusicPlaylistItem) -> String {
    let owner = p.channel.as_ref().map_or("YouTube Music", |c| c.name.as_str());
    match p.track_count {
        Some(n) => tf!("{} · {} faixas", owner, n),
        None => owner.to_string(),
    }
}

/// "Bom dia / Boa tarde / Boa noite" by local time.
fn greeting() -> &'static str {
    let hour = local_hour();
    match hour {
        5..=11 => tr("Bom dia"),
        12..=17 => tr("Boa tarde"),
        _ => tr("Boa noite"),
    }
}

fn local_hour() -> i32 {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: localtime_r writes into our own tm; time(NULL) has no pointer arguments.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&t, &mut tm).is_null() {
            return tm.tm_hour;
        }
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    ((secs / 3600) % 24) as i32
}

fn heading(ui: &mut egui::Ui, text: &str, size: f32) {
    ui.label(RichText::new(text).font(ui::bold(size)).color(c().text));
}

fn weak(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(c().text2));
}

/// Opens a folder in the system file manager.
fn open_path(path: &std::path::Path) {
    std::fs::create_dir_all(path).ok();
    let cmd = if cfg!(target_os = "windows") { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    crate::settings::command(cmd).arg(path).spawn().ok();
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("Espaço", "Tocar / pausar"),
    ("Ctrl + ← / →", "Faixa anterior / próxima"),
    ("Shift + ← / →", "Voltar / avançar 10 s"),
    ("← / →", "Voltar / avançar 5 s"),
    ("Ctrl + ↑ / ↓", "Volume"),
    ("M", "Mudo"),
    ("B", "Curtir a faixa atual"),
    ("S / R", "Aleatório / repetir"),
    ("Q", "Fila"),
    ("L", "Letra"),
    ("Ctrl + F ou /", "Buscar"),
    ("Ctrl + B", "Mostrar / esconder barra lateral"),
    ("Alt + ← / →", "Voltar / avançar entre telas"),
    ("Ctrl + H / Ctrl + L", "Início / músicas curtidas"),
    ("Ctrl + Shift + A / B", "Artista / álbum da faixa atual"),
    ("Ctrl + M", "Mini player"),
    ("Ctrl + Shift + K", "Vídeo"),
    ("F", "Tela cheia (no vídeo)"),
    ("Ctrl + ,", "Configurações"),
    ("? ou Ctrl + /", "Esta lista"),
    ("Ctrl + Q", "Sair"),
];

/// Shortcut results that depend on app state, resolved after input is read.
#[allow(clippy::large_enum_variant)] // a handful per frame, gone right after
enum Intent {
    Act(Act),
    PlayingArtist,
    PlayingAlbum,
    LikePlaying,
    /// Q / L / Ctrl+Shift+K: open the page, or go back if it's already open.
    Toggle(View),
    /// L: lyrics side panel on / off (or leave the full-window lyrics).
    Lyrics,
    Escape,
}

impl App {
    pub(crate) fn draw(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.shortcuts(ui.ctx());
        // Video decoding only while the video view (or its full screen) is showing.
        let video = !self.mini && (self.view == View::Video || self.vis_full) && self.current().is_some_and(|t| t.track_type == rustypipe::model::TrackType::Video);
        self.set_video(video);
        // The capture process (Winamp analyser) runs only while something consumes it.
        #[cfg(target_os = "linux")]
        if !self.mini {
            self.pw = None;
        }
        let p = c();
        if self.mini {
            if !self.s.winamp_skin.is_empty() && self.load_skin() {
                egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::BLACK)).show(ui, |ui| self.winamp_player(ui));
            } else {
                egui::CentralPanel::default().frame(egui::Frame::NONE.fill(p.bar)).show(ui, |ui| self.deck(ui));
            }
            return;
        }
        if self.vis_full {
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::BLACK)).show(ui, |ui| {
                let r = ui.max_rect();
                self.paint_video(ui, frame, r);
                if ui.interact(r, ui.id().with("vis-full"), egui::Sense::click()).double_clicked() {
                    self.exit_vis_full();
                }
            });
            return;
        }

        let since = (self.now() - self.shown_at) as f32;
        egui::Panel::bottom("player")
            .exact_size(72.0)
            .frame(egui::Frame::NONE.fill(p.bar))
            .show_separator_line(false)
            .show(ui, |ui| self.player_bar(ui));
        let lyrics_full = self.view == View::Lyrics;
        if lyrics_full {
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_gray(8))).show(ui, |ui| self.lyrics_full(ui));
            self.status_toast(ui.ctx());
            return;
        }
        if self.lyrics_panel {
            egui::Panel::right("lyrics")
                .resizable(true)
                .default_size(360.0)
                .size_range(280.0..=520.0)
                .frame(egui::Frame::NONE.fill(p.bar).inner_margin(egui::Margin { left: 16, right: 12, top: 14, bottom: 0 }))
                .show(ui, |ui| self.lyrics_side(ui));
        }
        if self.sidebar_open {
            egui::Panel::left("side")
                .resizable(true)
                .default_size(240.0)
                .size_range(200.0..=360.0)
                .frame(egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin::symmetric(8, 0)))
                .show(ui, |ui| self.sidebar(ui));
        }
        let central = egui::Frame::NONE.fill(p.bg).inner_margin(egui::Margin { left: 32, right: 32, top: 16, bottom: 0 });
        egui::CentralPanel::default().frame(central).show(ui, |ui| {
            self.page_tint(ui);
            self.top_bar(ui);
            ui::appear(ui, since);
            match self.view {
                View::Home => self.home(ui),
                View::Results => self.results(ui, since),
                View::List => self.list_page(ui, since),
                View::Artist => self.artist_page(ui),
                View::Queue => self.queue_page(ui, since),
                View::Lyrics => {}
                View::Video => self.video_page(ui, frame),
                View::Settings => self.settings_page(ui),
                View::Login => self.login(ui),
            }
        });
        if since < 1.0 && !self.s.reduce_motion {
            ui.ctx().request_repaint();
        }
        if self.show_shortcuts {
            self.shortcuts_modal(ui.ctx());
        }
        self.status_toast(ui.ctx());
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers as M};
        let typing = ctx.egui_wants_keyboard_input();
        let mut out: Vec<Intent> = vec![];
        let m = self.mpv;
        let (logged_in, mini) = (self.logged_in, self.mini);
        let (mut focus, mut sidebar, mut help, mut full) = (false, false, false, false);
        ctx.input_mut(|i| {
            if i.pointer.button_pressed(egui::PointerButton::Extra1) {
                out.push(Intent::Act(Act::Back));
            }
            if i.pointer.button_pressed(egui::PointerButton::Extra2) {
                out.push(Intent::Act(Act::Forward));
            }
            let ctrl_shift = M::COMMAND | M::SHIFT;
            let mut k = |mods: M, key: Key| i.consume_key(mods, key);
            // Modifier combos first, so a plain-key binding can't swallow them.
            if k(ctrl_shift, Key::A) {
                out.push(Intent::PlayingArtist);
            }
            if k(ctrl_shift, Key::B) {
                out.push(Intent::PlayingAlbum);
            }
            if k(ctrl_shift, Key::K) {
                out.push(Intent::Toggle(View::Video));
            }
            let mpv_keys: [(M, Key, &str, &[&str]); 6] = [
                (M::COMMAND, Key::ArrowLeft, "playlist-prev", &[]),
                (M::COMMAND, Key::ArrowRight, "playlist-next", &[]),
                (M::SHIFT, Key::ArrowLeft, "seek", &["-10"]),
                (M::SHIFT, Key::ArrowRight, "seek", &["10"]),
                (M::COMMAND, Key::ArrowUp, "add", &["volume", "5"]),
                (M::COMMAND, Key::ArrowDown, "add", &["volume", "-5"]),
            ];
            for (mods, key, cmd, args) in mpv_keys {
                if k(mods, key) {
                    m.command(cmd, args).ok();
                }
            }
            if k(M::ALT, Key::ArrowLeft) {
                out.push(Intent::Act(Act::Back));
            }
            if k(M::ALT, Key::ArrowRight) {
                out.push(Intent::Act(Act::Forward));
            }
            focus |= k(M::COMMAND, Key::F);
            sidebar |= k(M::COMMAND, Key::B);
            help |= k(M::COMMAND, Key::Slash);
            if k(M::COMMAND, Key::H) {
                out.push(Intent::Act(Act::Go(View::Home)));
            }
            if k(M::COMMAND, Key::L) && logged_in {
                out.push(Intent::Act(Act::Liked));
            }
            if k(M::COMMAND, Key::M) {
                out.push(Intent::Act(Act::Mini(!mini)));
            }
            if k(M::COMMAND, Key::Comma) {
                out.push(Intent::Act(Act::Go(View::Settings)));
            }
            if k(M::COMMAND, Key::Q) {
                out.push(Intent::Act(Act::Quit));
            }
            if k(M::NONE, Key::Escape) {
                out.push(Intent::Escape);
            }
            if typing {
                return;
            }
            let plain: [(Key, &str, &[&str]); 4] = [
                (Key::Space, "cycle", &["pause"]),
                (Key::ArrowLeft, "seek", &["-5"]),
                (Key::ArrowRight, "seek", &["5"]),
                (Key::M, "cycle", &["mute"]),
            ];
            for (key, cmd, args) in plain {
                if k(M::NONE, key) {
                    m.command(cmd, args).ok();
                }
            }
            if k(M::NONE, Key::S) {
                out.push(Intent::Act(Act::Shuffle));
            }
            if k(M::NONE, Key::R) {
                out.push(Intent::Act(Act::Repeat));
            }
            if k(M::NONE, Key::B) {
                out.push(Intent::LikePlaying);
            }
            if k(M::NONE, Key::Q) {
                out.push(Intent::Toggle(View::Queue));
            }
            if k(M::NONE, Key::L) {
                out.push(Intent::Lyrics);
            }
            full |= k(M::NONE, Key::F);
            focus |= k(M::NONE, Key::Slash);
            help |= k(M::SHIFT, Key::Questionmark) || k(M::NONE, Key::Questionmark);
        });
        self.focus_search |= focus;
        self.sidebar_open ^= sidebar;
        self.show_shortcuts ^= help;
        if full && self.view == View::Video {
            self.vis_full = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        for intent in out {
            let act = match intent {
                Intent::Act(a) => Some(a),
                Intent::PlayingArtist => self.current().and_then(|t| t.artists.iter().find_map(|a| a.id.clone())).map(Act::Artist),
                Intent::PlayingAlbum => self.current().and_then(|t| t.album.clone()).map(|a| Act::Album(a.id, a.name)),
                Intent::LikePlaying => self.current().cloned().filter(|_| self.logged_in).map(Act::Like),
                Intent::Toggle(v) if self.view == v => Some(Act::Back),
                Intent::Toggle(v) => Some(Act::Go(v)),
                Intent::Lyrics => self.toggle_lyrics(),
                Intent::Escape => {
                    if self.show_shortcuts {
                        self.show_shortcuts = false;
                    } else if self.vis_full {
                        self.exit_vis_full();
                    } else if self.view == View::Lyrics {
                        return self.acts.push(Act::Back);
                    }
                    None
                }
            };
            self.acts.extend(act);
        }
    }

    /// Status line as a toast above the player bar: errors stay 10 s, notes 5 s; click to dismiss.
    fn status_toast(&mut self, ctx: &egui::Context) {
        let p = c();
        let now = self.now();
        if self.status != self.status_seen {
            self.status_seen = self.status.clone();
            self.status_at = now;
        }
        let error = self.status.starts_with(tr("Erro"));
        let ttl = if error { 10.0 } else { 5.0 };
        if self.status.is_empty() || now - self.status_at > ttl {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let r = egui::Area::new(egui::Id::new("status-toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -88.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(p.selected)
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .shadow(egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(120) })
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        let col = if error { ui.visuals().error_fg_color } else { p.text };
                        ui.label(RichText::new(&self.status).color(col));
                    })
            });
        if r.response.interact(egui::Sense::click()).clicked() {
            self.status.clear();
        }
    }

    fn exit_vis_full(&mut self) {
        self.vis_full = false;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    }

    fn shortcuts_modal(&mut self, ctx: &egui::Context) {
        let r = egui::Modal::new(egui::Id::new("shortcuts")).show(ctx, |ui| {
            ui.set_width(420.0);
            heading(ui, tr("Atalhos de teclado"), 20.0);
            ui.add_space(12.0);
            egui::Grid::new("keys").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                for (k, what) in SHORTCUTS {
                    ui.label(RichText::new(tr(k)).font(ui::medium(13.0)).color(c().text));
                    weak(ui, tr(what));
                    ui.end_row();
                }
            });
            ui.add_space(12.0);
            weak(ui, tr("No Linux, o playerctl controla o YtmFast pelo terminal (playerctl -p ytmfast play-pause)."));
        });
        if r.should_close() {
            self.show_shortcuts = false;
        }
    }

    // ---- video ----

    /// Draws the current video frame into `rect` (mpv letterboxes it to keep the aspect).
    /// mpv's video output (vo=libmpv) only works once the render context exists: it has to be there
    /// before the picture arrives, or mpv drops the video track.
    fn ensure_vis(&mut self) {
        if self.vis.is_none() {
            self.vis = self.gpa.clone().and_then(|g| crate::vis::Vis::new(self.mpv, g, self.ctx.clone()));
        }
    }

    fn paint_video(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame, rect: Rect) {
        let Some(gl) = frame.gl().cloned() else { return };
        self.ensure_vis();
        let Some(vis) = self.vis.as_mut() else { return };
        let ppp = ui.ctx().pixels_per_point();
        let (tex, _) = vis.frame(&gl, frame, [(rect.width() * ppp) as i32, (rect.height() * ppp) as i32]);
        ui.painter().image(tex, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }

    /// Pulls the newest captured audio (PipeWire) into `self.pcm` for the Winamp analyser.
    pub(crate) fn pull_tap(&mut self) {
        self.pcm.clear();
        #[cfg(target_os = "linux")]
        if self.pw_ok {
            let now = self.now();
            if self.pw.as_mut().is_some_and(|p| !p.alive()) {
                self.pw = None;
            }
            // Our node only exists while mpv plays; retry gently until it shows up.
            if self.pw.is_none() && self.pl.pos >= 0 && now - self.pw_retry > 2.0 {
                self.pw_retry = now;
                self.pw = crate::pcm::PwTap::start();
            }
            if let Some(p) = &self.pw {
                self.pcm = p.take(crate::pcm::volume_gain(self.pl.vol));
            }
        }
    }

    /// Loads (or reuses) the selected Winamp skin. On failure falls back to the modern mini player.
    fn load_skin(&mut self) -> bool {
        let name = self.s.winamp_skin.clone();
        // The themed skin is redrawn when the theme changes.
        let key = if name == crate::winamp::THEMED {
            let p = crate::theme::c();
            format!("{name}{:?}{:?}{:?}", p.bg, p.text, p.accent)
        } else {
            name.clone()
        };
        if self.skin.as_ref().is_some_and(|(n, _)| *n == key) {
            return true;
        }
        match crate::winamp::Skin::load(&self.ctx, &name) {
            Ok(sk) => {
                self.skin = Some((key, sk));
                true
            }
            Err(e) => {
                self.status = tf!("Erro na skin {}: {}", name, e);
                self.skin = None;
                self.s.winamp_skin.clear();
                self.acts.push(Act::Mini(true));
                false
            }
        }
    }

    fn winamp_player(&mut self, ui: &mut egui::Ui) {
        self.pull_tap();
        // Frames without new audio keep the bars; silence lets them fall.
        if !self.pcm.is_empty() || self.pl.paused || self.pl.pos < 0 {
            crate::winamp::analyse(&self.pcm, &mut self.bars);
        }
        let m = self.mpv;
        let title = match self.current() {
            Some(t) => format!("{}. {} - {} ({})", self.pl.pos + 1, ui::artists(t), t.name, ui::fmt_time(self.pl.dur)),
            None => "YtmFast".into(),
        };
        let st = crate::winamp::State {
            title: &title,
            time: self.pl.time,
            dur: self.pl.dur,
            playing: self.pl.pos >= 0,
            paused: self.pl.paused,
            volume: (self.pl.vol / 100.0) as f32,
            balance: self.s.balance,
            shuffle: self.s.shuffle,
            repeat: self.s.repeat != Repeat::Off,
            kbps: (m.get_property::<f64>("audio-bitrate").unwrap_or(0.0) / 1000.0) as u32,
            khz: (m.get_property::<i64>("audio-params/samplerate").unwrap_or(0) / 1000) as u32,
            stereo: m.get_property::<i64>("audio-params/channel-count").unwrap_or(2) >= 2,
            bars: &self.bars,
        };
        let Some((_, skin)) = &self.skin else { return };
        let events = crate::winamp::draw(ui, skin, &st);
        if self.pl.pos >= 0 && !self.pl.paused || self.bars.iter().any(|b| *b > 0.0) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        use crate::winamp::Event as E;
        for e in events {
            match e {
                E::Prev => drop(m.command("playlist-prev", &[])),
                E::Next => drop(m.command("playlist-next", &[])),
                E::Play => {
                    if self.pl.pos < 0 && !self.queue.is_empty() {
                        m.command("playlist-play-index", &["0"]).ok();
                    }
                    m.set_property("pause", false).ok();
                }
                E::Pause => drop(m.command("cycle", &["pause"])),
                E::Stop => {
                    m.set_property("pause", true).ok();
                    m.command("seek", &["0", "absolute"]).ok();
                }
                E::Exit => self.acts.push(Act::Mini(false)),
                E::Minimize => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                E::Seek(f) => drop(m.command("seek", &[&(f as f64 * self.pl.dur).to_string(), "absolute"])),
                E::Volume(v) => {
                    m.set_property("volume", v as f64 * 100.0).ok();
                    self.s.volume = v as f64 * 100.0;
                }
                E::Balance(b) => self.s.balance = b,
                E::Shuffle => self.acts.push(Act::Shuffle),
                E::Repeat => self.s.repeat = if self.s.repeat == Repeat::Off { Repeat::All } else { Repeat::Off },
            }
        }
    }

    // ---- chrome ----

    /// Album-art colour washing down from the top of album / playlist / artist pages.
    fn page_tint(&mut self, ui: &mut egui::Ui) {
        let cover = match self.view {
            View::List => self.list.cover.clone(),
            View::Artist => self.artist.as_ref().map(|a| a.header_image.clone()).unwrap_or_default(),
            View::Lyrics => self.current().map(|t| t.cover.clone()).unwrap_or_default(),
            // Home picks up the colour of whatever is playing.
            View::Home => match self.current() {
                Some(t) => t.cover.clone(),
                None => return,
            },
            _ => return,
        };
        let size = match self.view {
            View::Lyrics => 280.0,
            View::Home => 40.0,
            _ => 200.0,
        };
        if let Some(col) = self.accent_for(&cover, size) {
            let full = ui.max_rect().expand2(vec2(32.0, 16.0));
            let r = Rect::from_min_size(full.min, vec2(full.width(), 420.0));
            let strength = if c().dark { 0.38 } else { 0.22 };
            ui::gradient(ui, r, col.gamma_multiply(strength), Color32::TRANSPARENT);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let nav = |ui: &mut egui::Ui, icon, enabled: bool| {
                ui.add_enabled_ui(enabled, |ui| {
                    if !enabled {
                        ui.multiply_opacity(0.35);
                    }
                    ui::icon_button(ui, icon, 34.0)
                })
                .inner
                .clicked()
            };
            if nav(ui, Icon::Back, !self.back.is_empty()) {
                self.acts.push(Act::Back);
            }
            if nav(ui, Icon::Forward, !self.fwd.is_empty()) {
                self.acts.push(Act::Forward);
            }
            ui.add_space(8.0);
            self.search_box(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.account_button(ui);
                if ui::icon_toggle(ui, Icon::Gear, 36.0, self.view == View::Settings).on_hover_text(tr("Configurações (Ctrl+,)")).clicked() {
                    self.acts.push(Act::Go(View::Settings));
                }
                if ui::icon_button(ui, Icon::Mini, 36.0).on_hover_text(tr("Mini player (Ctrl+M)")).clicked() {
                    self.acts.push(Act::Mini(true));
                }
                let on = self.view == View::Video;
                if ui::icon_toggle(ui, Icon::Video, 36.0, on).on_hover_text(tr("Vídeo (Ctrl+Shift+K)")).clicked() {
                    self.acts.push(if on { Act::Back } else { Act::Go(View::Video) });
                }
            });
        });
    }

    /// Account avatar (photo or initial) with a menu; a sign-in button when signed out.
    fn account_button(&mut self, ui: &mut egui::Ui) {
        let p = c();
        if !self.logged_in {
            if ui::pill(ui, Some(Icon::Person), tr("Entrar"), true).clicked() {
                self.acts.push(Act::Go(View::Login));
            }
            return;
        }
        let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::click());
        let avatar = rect.shrink(2.0);
        match &self.account {
            Some(a) if !a.photo.is_empty() => ui::paint_cover(ui, &a.photo, avatar, 16),
            _ => {
                ui.painter().circle_filled(avatar.center(), 16.0, p.accent);
                let initial = self.account.as_ref().and_then(|a| a.name.chars().next()).map_or("?".to_string(), |c| c.to_uppercase().to_string());
                ui.painter().text(avatar.center(), egui::Align2::CENTER_CENTER, initial, ui::bold(15.0), Color32::WHITE);
            }
        }
        if resp.hovered() {
            ui.painter().circle_stroke(avatar.center(), 17.0, egui::Stroke::new(2.0, p.text2));
        }
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            if let Some(a) = &self.account {
                ui.label(RichText::new(&a.name).font(ui::bold(15.0)).color(p.text));
                if !a.handle.is_empty() {
                    weak(ui, &a.handle);
                }
            } else {
                weak(ui, tr("Conectado ao YouTube Music"));
            }
            if !self.s.login_browser.is_empty() {
                weak(ui, format!("via {}", crate::login::label(&self.s.login_browser)));
            }
            ui.separator();
            if ui.button(tr("Trocar de conta")).clicked() {
                self.acts.push(Act::Logout);
                self.acts.push(Act::Go(View::Login));
            }
            if ui.button(tr("Sair")).clicked() {
                self.acts.push(Act::Logout);
            }
        });
    }

    fn search_box(&mut self, ui: &mut egui::Ui) {
        let p = c();
        // Leave room for the buttons on the right (account button is wider when signed out).
        let right = if self.logged_in { 200.0 } else { 290.0 };
        let w = (ui.available_width() - right - 60.0).clamp(90.0, 420.0);
        let hint = if w < 330.0 { tr("Pesquisar") } else { tr("Pesquisar músicas, artistas, álbuns e playlists") };
        egui::Frame::NONE.fill(p.bar).corner_radius(8).inner_margin(egui::Margin::symmetric(14, 9)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::hover());
                ui::paint_icon(ui.painter(), Icon::Search, r, p.text2);
                let te = egui::TextEdit::singleline(&mut self.query)
                    .frame(egui::Frame::NONE)
                    .font(ui::regular(15.0))
                    .hint_text(hint)
                    .desired_width(w);
                let r = ui.add(te);
                if std::mem::take(&mut self.focus_search) {
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.query.trim().is_empty() {
                    self.search();
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = c();
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::hover());
            ui::paint_logo(ui.painter(), r);
            ui.label(RichText::new("YtmFast").font(ui::bold(20.0)).color(p.text));
        });
        ui.add_space(18.0);
        let cur = self.view;
        let mut nav = |ui: &mut egui::Ui, icon, label, view: View| {
            if ui::nav_item(ui, icon, label, cur == view).clicked() {
                self.acts.push(Act::Go(view));
            }
        };
        nav(ui, Icon::Home, tr("Início"), View::Home);
        nav(ui, Icon::Queue, tr("Fila"), View::Queue);
        if !self.logged_in {
            nav(ui, Icon::Person, tr("Entrar"), View::Login);
            return;
        }
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);

        // Library header: title, filter search, new playlist.
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(RichText::new(tr("Biblioteca")).font(ui::bold(16.0)).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::icon_button(ui, Icon::Plus, 30.0).on_hover_text(tr("Nova playlist")).clicked() {
                    self.new_playlist = Some((String::new(), vec![]));
                }
                if ui::icon_toggle(ui, Icon::Search, 30.0, self.lib_search).on_hover_text(tr("Filtrar biblioteca")).clicked() {
                    self.lib_search = !self.lib_search;
                    self.lib_query.clear();
                }
            });
        });
        if matches!(&self.new_playlist, Some((_, ids)) if ids.is_empty()) {
            self.new_playlist_input(ui, false);
        }
        if self.lib_search {
            ui.add(egui::TextEdit::singleline(&mut self.lib_query).hint_text(tr("Filtrar por nome")).desired_width(f32::INFINITY));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            for (i, label) in [(1u8, tr("Playlists")), (2, tr("Álbuns"))] {
                if ui::chip(ui, label, self.lib_filter == i).clicked() {
                    self.lib_filter = if self.lib_filter == i { 0 } else { i };
                }
            }
        });
        ui.add_space(6.0);

        let compact = self.s.sidebar_compact;
        let q = self.lib_query.to_lowercase();
        let matches = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let open = self.view == View::List;
            if self.lib_filter != 2 && matches(tr("Músicas curtidas")) {
                let sel = open && self.list.kind == ListKind::Liked;
                let sub = tf!("Playlist automática · {} faixas", self.liked_tracks.len());
                if ui::lib_item(ui, ui::Art::Liked, tr("Músicas curtidas"), &sub, sel, compact).clicked() {
                    self.acts.push(Act::Liked);
                }
            }
            if self.lib_filter != 2 {
                for pl in self.lib.0.iter().filter(|pl| matches(&pl.name)) {
                    if ui::lib_item(ui, ui::Art::Cover(&pl.thumbnail), &pl.name, &playlist_sub(pl), open && self.list.id == pl.id, compact).clicked() {
                        self.acts.push(Act::Playlist(pl.id.clone(), pl.name.clone()));
                    }
                }
            }
            if self.lib_filter != 1 {
                for a in self.lib.1.iter().filter(|a| matches(&a.name)) {
                    if ui::lib_item(ui, ui::Art::Cover(&a.cover), &a.name, &tf!("Álbum · {}", album_sub(a)), open && self.list.id == a.id, compact).clicked() {
                        self.acts.push(Act::Album(a.id.clone(), a.name.clone()));
                    }
                }
            }
        });
    }

    /// "Nova playlist" entry; `from_queue` seeds it with the upcoming queue.
    fn new_playlist_input(&mut self, ui: &mut egui::Ui, from_queue: bool) {
        match &mut self.new_playlist {
            Some((name, ids)) if ids.is_empty() != from_queue => {
                let r = ui.add(egui::TextEdit::singleline(name).hint_text(tr("Nome da playlist")).desired_width(if from_queue { 260.0 } else { f32::INFINITY }));
                r.request_focus();
                if r.lost_focus() {
                    let (name, ids) = (std::mem::take(name), std::mem::take(ids));
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.trim().is_empty() {
                        self.acts.push(Act::CreatePlaylist(name.trim().to_string(), ids));
                    }
                    self.new_playlist = None;
                }
            }
            _ if from_queue
                && ui::pill(ui, Some(Icon::Plus), tr("Salvar como playlist"), false).clicked() => {
                    let start = self.pl.pos.max(0) as usize;
                    let ids = self.queue.iter().skip(start).map(|t| t.id.clone()).collect();
                    self.new_playlist = Some((String::new(), ids));
                }
            _ => {}
        }
    }

    // ---- player ----

    /// Full-width progress line on the top edge of `full`. Returns the time to display.
    fn progress_line(&mut self, ui: &mut egui::Ui, full: Rect) -> f64 {
        let (time, dur) = (self.pl.time, self.pl.dur);
        let frac = if dur > 0.0 { (time / dur) as f32 } else { 0.0 };
        let hit = Rect::from_min_max(full.left_top() - vec2(0.0, 6.0), pos2(full.right(), full.top() + 6.0));
        // The knob pokes above the bar's top edge: draw on a layer over the panels, unclipped,
        // or the bar's clip and the page painted afterwards cut it in half.
        let layer = egui::LayerId::new(egui::Order::Middle, ui.id().with("seek-layer"));
        // "Most replayed" heat, fetched once per song while a window shows it.
        if let Some(t) = self.current()
            && self.heat.as_ref().is_none_or(|(id, _)| *id != t.id)
        {
            let id = t.id.clone();
            self.heat = Some((id.clone(), None));
            self.spawn(async move { Ok(crate::Msg::Heat(id.clone(), tokio::task::spawn_blocking(move || crate::heat::fetch(&id)).await.ok().flatten())) });
        }
        let heat = self.heat.as_ref().and_then(|(_, h)| h.clone());
        let mut jump = None;
        let (r, f) = ui
            .scope_builder(egui::UiBuilder::new().layer_id(layer).max_rect(hit), |ui| {
                ui.set_clip_rect(ui.ctx().content_rect());
                // The ridge fades in while the bar is hovered, as on YouTube.
                let ridge = Rect::from_min_max(pos2(full.left(), full.top() - 18.0), pos2(full.right(), full.top()));
                // By position: this floating layer isn't one egui's layer hit-test knows about.
                let near = ui.ctx().pointer_hover_pos().is_some_and(|q| ridge.union(hit).contains(q));
                let shown = ui.ctx().animate_bool_with_time(ui.id().with("heat-shown"), near && heat.is_some(), if crate::theme::look().motion { 0.15 } else { 0.0 });
                if let Some(h) = heat.as_ref().filter(|_| shown > 0.01) {
                    let p = c();
                    let x = |t: f64| full.left() + (t / h.length).clamp(0.0, 1.0) as f32 * full.width();
                    let y = |v: f32| full.top() - 1.0 - v * 14.0;
                    let mut mesh = egui::Mesh::default();
                    let col = p.text.gamma_multiply(0.28 * shown);
                    for &(t, v) in &h.points {
                        mesh.colored_vertex(pos2(x(t), full.top()), col);
                        mesh.colored_vertex(pos2(x(t), y(v)), col);
                    }
                    for i in 0..h.points.len().saturating_sub(1) as u32 {
                        mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
                        mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
                    }
                    ui.painter().add(egui::Shape::mesh(mesh));
                    // A thin edge along the top gives the ridge its shape over busy artwork.
                    let edge: Vec<egui::Pos2> = h.points.iter().map(|&(t, v)| pos2(x(t), y(v))).collect();
                    ui.painter().add(egui::Shape::line(edge, egui::Stroke::new(1.0, p.text.gamma_multiply(0.7 * shown))));
                    if let Some(peak) = h.peak {
                        let top = h.points.iter().min_by(|a, b| (a.0 - peak).abs().total_cmp(&(b.0 - peak).abs())).map_or(0.0, |p| p.1);
                        let c0 = pos2(x(peak), y(top) - 4.0);
                        let resp = ui.interact(Rect::from_center_size(c0, vec2(14.0, 14.0)), ui.id().with("heat-peak"), egui::Sense::click());
                        ui.painter().circle_filled(c0, if resp.hovered() { 4.5 } else { 3.5 }, p.accent.gamma_multiply(shown));
                        if resp.on_hover_text(tr("Trecho mais repetido · clique para ir")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            jump = Some(peak);
                        }
                    }
                }
                ui::seek_bar(ui, hit, "seek", frac, c().accent)
            })
            .inner;
        if let Some(t) = jump {
            self.mpv.command("seek", &[&format!("{t:.1}"), "absolute"]).ok();
        } else if dur > 0.0 && (r.drag_stopped() || r.clicked()) {
            self.mpv.command("seek", &[&(f as f64 * dur).to_string(), "absolute"]).ok();
        }
        if r.is_pointer_button_down_on() { f as f64 * dur } else { time }
    }

    fn transport(&mut self, ui: &mut egui::Ui, small: bool) {
        let s = if small { 34.0 } else { 40.0 };
        let paused = self.pl.paused || self.pl.pos < 0;
        if ui::icon_button(ui, Icon::Prev, s).clicked() {
            self.mpv.command("playlist-prev", &[]).ok();
        }
        if ui::play_button(ui, paused, s - 2.0).clicked() {
            self.mpv.command("cycle", &["pause"]).ok();
        }
        if ui::icon_button(ui, Icon::Next, s).clicked() {
            self.mpv.command("playlist-next", &[]).ok();
        }
    }

    fn player_bar(&mut self, ui: &mut egui::Ui) {
        let p = c();
        let full = ui.max_rect();
        let shown = self.progress_line(ui, full);
        let body = full.shrink2(vec2(16.0, 0.0));
        let row = |x0: f32, x1: f32| Rect::from_min_max(pos2(x0, body.top()), pos2(x1, body.bottom()));
        // Narrow windows drop the volume slider (the icon stays and scrolls) to keep the title readable.
        let compact = full.width() < 1000.0;
        let left = row(body.left(), body.left() + if compact { 215.0 } else { 250.0 });
        let right = row(body.right() - if compact { 250.0 } else { 390.0 }, body.right());
        let mid = row(left.right() + 12.0, right.left() - 12.0);
        let lay = |r: Rect, l: egui::Layout| egui::UiBuilder::new().max_rect(r).layout(l);
        let ltr = egui::Layout::left_to_right(egui::Align::Center);

        ui.scope_builder(lay(left, ltr), |ui| {
            self.transport(ui, false);
            ui.add_space(8.0);
            if self.pl.dur > 0.0 {
                ui.label(RichText::new(format!("{} / {}", ui::fmt_time(shown), ui::fmt_time(self.pl.dur))).color(p.text2).size(12.0));
            }
        });

        // Crossfade: the outgoing song rises and fades over the first half, the incoming one comes up
        // from below over the second (no overlap), in step with the audio. Otherwise what's playing.
        // The fade's position comes from the player clock in steps; eased through a short animation
        // (one per fade) it glides at the frame rate instead of hopping, and finishes its glide even
        // once the handoff (k = 1) has begun.
        let fade = self.xfade.filter(|(k, _)| *k > 0.0).and_then(|(k, from)| {
            let f = usize::try_from(from).ok()?;
            let k = ui.ctx().animate_value_with_time(ui.id().with(("xfade-k", from)), k, if self.s.reduce_motion { 0.0 } else { 0.25 });
            (k < 0.999).then(|| (k, self.queue.get(f).cloned(), self.queue.get(f + 1).cloned()))
        });
        if let Some((k, Some(old), Some(new))) = fade {
            ui.ctx().request_repaint();
            let smooth = |a: f32, b: f32| {
                let x = ((k - a) / (b - a)).clamp(0.0, 1.0);
                x * x * (3.0 - 2.0 * x)
            };
            // The two overlap in the middle, like the sound: the old one rises and fades out, the new
            // one comes up from below as it fades in.
            let (out, inn) = (smooth(0.0, 0.6), smooth(0.4, 1.0));
            let lift = 14.0;
            for (t, alpha, dy) in [(old, 1.0 - out, -lift * out), (new, inn, lift * (1.0 - inn))] {
                if alpha > 0.0 {
                    ui.scope(|ui| {
                        ui.set_clip_rect(mid);
                        ui.multiply_opacity(alpha);
                        self.now_playing(ui, mid.translate(vec2(0.0, dy)), &t);
                    });
                }
            }
        } else if let Some(t) = self.current().cloned() {
            self.now_playing(ui, mid, &t);
        }

        ui.scope_builder(lay(right, egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if ui::icon_toggle(ui, Icon::Queue, 38.0, self.view == View::Queue).on_hover_text(tr("Fila (Q)")).clicked() {
                self.acts.push(if self.view == View::Queue { Act::Back } else { Act::Go(View::Queue) });
            }
            if ui::icon_toggle(ui, Icon::Mic, 38.0, self.lyrics_panel || self.view == View::Lyrics).on_hover_text(tr("Letra (L)")).clicked() {
                let act = self.toggle_lyrics();
                self.acts.extend(act);
            }
            ui.add_space(6.0);
            self.volume(ui, compact);
            ui.add_space(6.0);
            let rep_icon = if self.s.repeat == Repeat::One { Icon::RepeatOne } else { Icon::Repeat };
            if ui::icon_toggle(ui, rep_icon, 38.0, self.s.repeat != Repeat::Off).on_hover_text(tr("Repetir (R)")).clicked() {
                self.acts.push(Act::Repeat);
            }
            if ui::icon_toggle(ui, Icon::Shuffle, 38.0, self.s.shuffle).on_hover_text(tr("Aleatório (S)")).clicked() {
                self.acts.push(Act::Shuffle);
            }
        });
    }

    /// Cover, title and artist in the middle of the player bar.
    fn now_playing(&mut self, ui: &mut egui::Ui, mid: Rect, t: &TrackItem) {
        let p = c();
        let ltr = egui::Layout::left_to_right(egui::Align::Center);
        ui.scope_builder(egui::UiBuilder::new().max_rect(mid).layout(ltr), |ui| {
            let max_text = (mid.width() - 110.0).clamp(40.0, 420.0);
            let title = ui::line(ui, &t.name, ui::medium(14.0), p.text, max_text);
            let mut sub = ui::artists(t);
            if let Some(a) = &t.album {
                sub = format!("{sub} • {}", a.name);
            }
            let sub = ui::line(ui, &sub, ui::regular(13.0), p.text2, max_text);
            let w = 56.0 + title.size().x.max(sub.size().x) + 48.0;
            ui.add_space(((mid.width() - w) / 2.0).max(0.0));
            let (cr, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::click());
            ui::tilt_cover(ui, &resp, cr, &t.cover, 2, 0.35);
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr("Letra")).clicked() {
                self.acts.push(Act::Go(View::Lyrics));
                self.load_lyrics();
            }
            ui.add_space(8.0);
            let tw = title.size().x.max(sub.size().x);
            let (text_r, tresp) = ui.allocate_exact_size(vec2(tw, 40.0), egui::Sense::click());
            let painter = ui.painter();
            painter.galley(pos2(text_r.left(), text_r.center().y - 19.0), title, p.text);
            painter.galley(pos2(text_r.left(), text_r.center().y + 1.0), sub, p.text2);
            // Click the text: go to the artist (or the album when there's no artist page).
            if tresp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                if let Some(id) = t.artists.iter().find_map(|a| a.id.clone()) {
                    self.acts.push(Act::Artist(id));
                } else if let Some(a) = &t.album {
                    self.acts.push(Act::Album(a.id.clone(), a.name.clone()));
                }
            }
            if self.logged_in && ui::heart_button(ui, self.liked.contains(&t.id)).on_hover_text(tr("Curtir (B)")).clicked() {
                self.acts.push(Act::Like(t.clone()));
            }
        });
    }

    fn volume(&mut self, ui: &mut egui::Ui, compact: bool) {
        let vol = self.pl.vol;
        let muted: bool = self.mpv.get_property("mute").unwrap_or(false);
        let mut over_bar = false;
        if !compact {
            let (rect, _) = ui.allocate_exact_size(vec2(90.0, 24.0), egui::Sense::hover());
            let (r, f) = ui::seek_bar(ui, rect, "vol", if muted { 0.0 } else { (vol / 100.0) as f32 }, c().text);
            over_bar = r.hovered();
            if r.is_pointer_button_down_on() {
                self.mpv.set_property("mute", false).ok();
                self.mpv.set_property("volume", f as f64 * 100.0).ok();
            }
            if r.drag_stopped() || r.clicked() {
                self.s.volume = f as f64 * 100.0;
            }
        }
        let icon = if muted || vol < 1.0 { Icon::Mute } else { Icon::Volume };
        let resp = ui::icon_button(ui, icon, 38.0).on_hover_text(tf!("Volume {}% · roda do mouse ajusta · clique: mudo (M)", format!("{:.0}", vol)));
        // Mouse wheel over the icon or the slider changes the volume.
        let scroll = if resp.hovered() || over_bar { ui.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
        if scroll != 0.0 {
            let v = (vol + scroll as f64 * 0.1).clamp(0.0, 100.0);
            self.mpv.set_property("volume", v).ok();
            self.s.volume = v;
        }
        if resp.clicked() {
            self.mpv.command("cycle", &["mute"]).ok();
        }
    }

    // ---- rows ----

    fn row_ctx<'a>(&'a self, t: &TrackItem, removable: bool, show_album: bool) -> RowCtx<'a> {
        let playlists: &[rustypipe::model::MusicPlaylistItem] = if self.logged_in { &self.lib.0 } else { &[] };
        RowCtx { liked: self.liked.contains(&t.id), playlists, removable, compact: self.s.tracklist_compact, show_album, movable: false }
    }

    /// Maps a row click to an action. Move / drag clicks need the row index: callers handle them.
    fn row_act(&self, click: RowClick, t: &TrackItem, play: Act, remove: Act) -> Option<Act> {
        Some(match click {
            RowClick::MoveUp | RowClick::MoveDown | RowClick::DragStart => return None,
            RowClick::Play => play,
            RowClick::PlayNext => Act::PlayNext(t.clone()),
            RowClick::Enqueue => Act::Enqueue(t.clone()),
            RowClick::Radio => Act::Radio(t.clone()),
            RowClick::Like => Act::Like(t.clone()),
            RowClick::AddTo(i) => Act::AddTo(self.lib.0[i].id.clone(), t.clone()),
            RowClick::Remove => remove,
            RowClick::Artist(i) => Act::Artist(t.artists[i].id.clone().unwrap_or_default()),
            RowClick::Album => {
                let a = t.album.as_ref().expect("menu only shown with album");
                Act::Album(a.id.clone(), a.name.clone())
            }
        })
    }

    /// Virtualized track list. Reorderable lists (`opts.movable`) support drag and drop.
    fn track_list(&mut self, ui: &mut egui::Ui, tracks: &[TrackItem], since: f32, opts: TrackListOpts) {
        let row_h = if self.s.tracklist_compact { ui::ROW_H_COMPACT } else { ui::ROW_H };
        let playing_id = self.current().map(|t| t.id.clone());
        let off = opts.queue_offset;
        let mv = |from: usize, to: usize| match off {
            Some(o) => Act::QueueMove(from + o, to + o),
            None => Act::Move(from, to),
        };
        let dragging = self.drag.filter(|_| opts.movable);
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let mut drop_at: Option<usize> = None;
        let mut new_drag = None;
        let mut acts = vec![];
        egui::ScrollArea::vertical().id_salt(opts.id).auto_shrink(false).show_rows(ui, row_h, tracks.len(), |ui, range| {
            let first = range.start;
            for i in range {
                let t = &tracks[i];
                let playing = match off {
                    Some(o) => (i + o) as i64 == self.pl.pos,
                    None => playing_id.as_deref() == Some(t.id.as_str()),
                };
                let mut rc = self.row_ctx(t, opts.removable, opts.show_album);
                rc.movable = opts.movable;
                let (click, rect) = ui
                    .scope(|ui| {
                        // The dragged row fades; the others cascade in as usual.
                        ui.multiply_opacity(if dragging == Some(i) { 0.35 } else { ui::cascade(since, i - first) });
                        ui::track_row_at(ui, t, playing, &rc)
                    })
                    .inner;
                if let (Some(_), Some(p)) = (dragging, pointer)
                    && p.y >= rect.top()
                    && p.y < rect.bottom()
                {
                    // Drop above or below this row depending on which half the pointer is in.
                    let below = p.y > rect.center().y;
                    drop_at = Some(i + below as usize);
                    let y = if below { rect.bottom() } else { rect.top() };
                    ui.painter().line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], egui::Stroke::new(2.0, c().accent));
                }
                let Some(cl) = click else { continue };
                match cl {
                    RowClick::DragStart => new_drag = Some(i),
                    RowClick::MoveUp if i > 0 => acts.push(mv(i, i - 1)),
                    RowClick::MoveDown if i + 1 < tracks.len() => acts.push(mv(i, i + 1)),
                    RowClick::MoveUp | RowClick::MoveDown => {}
                    cl => {
                        let (play, remove) = match off {
                            Some(o) => (Act::Jump(i + o), Act::Unqueue(i + o)),
                            None if opts.solo => (Act::Radio(t.clone()), Act::Remove(i, t.clone())),
                            None => (Act::Play(tracks.to_vec(), i), Act::Remove(i, t.clone())),
                        };
                        acts.extend(self.row_act(cl, t, play, remove));
                    }
                }
            }
        });
        if let Some(i) = new_drag {
            self.drag = Some(i);
        }
        if let Some(from) = dragging {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if !ui.input(|i| i.pointer.primary_down()) {
                // Released: an insertion point "before row t" becomes a final index.
                if let Some(t) = drop_at {
                    let to = if t > from { t - 1 } else { t };
                    if to != from {
                        acts.push(mv(from, to));
                    }
                }
                self.drag = None;
            }
        }
        self.acts.extend(acts);
    }

    // ---- pages ----

    fn home(&mut self, ui: &mut egui::Ui) {
        let p = c();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(8.0);
            if !self.logged_in {
                egui::Frame::NONE.fill(p.bar).corner_radius(8).inner_margin(20).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    heading(ui, tr("Entre para ver sua biblioteca"), 18.0);
                    weak(ui, tr("Playlists, curtidas e áudio na qualidade Premium."));
                    ui.add_space(10.0);
                    if ui::pill(ui, Some(Icon::Person), tr("Entrar"), true).clicked() {
                        self.acts.push(Act::Go(View::Login));
                    }
                });
            }
            ui.add_space(12.0);
            heading(ui, greeting(), 32.0);
            ui.add_space(14.0);
            // Unique recent tracks, newest first.
            let mut seen = std::collections::HashSet::new();
            let recent: Vec<TrackItem> = self.history.iter().filter(|h| seen.insert(h.track.id.as_str())).take(20).map(|h| h.track.clone()).collect();
            self.quick_tiles(ui, &recent);
            // YouTube Music's own feed, in its order; the local shelves only fill in when it's missing.
            for sh in &self.home {
                if let Some(i) = ui::shelf_by(ui, &sh.title, &sh.items, false, |c| c.round(), |c| (&c.thumb, &c.title, c.sub.clone())) {
                    self.acts.push(sh.items[i].act());
                }
            }
            if self.home.is_empty()
                && let Some(i) = ui::shelf(ui, tr("Ouvir novamente"), &recent, false, false, |t| (&t.cover, &t.name, ui::artists(t)))
            {
                self.acts.push(Act::Radio(recent[i].clone()));
            }
            if let Some(i) = ui::shelf(ui, tr("Suas playlists"), &self.lib.0, true, false, |p| (&p.thumbnail, &p.name, playlist_sub(p))) {
                let pl = &self.lib.0[i];
                self.acts.push(Act::Playlist(pl.id.clone(), pl.name.clone()));
            }
            if let Some(i) = ui::shelf(ui, tr("Seus álbuns"), &self.lib.1, false, false, |a| (&a.cover, &a.name, album_sub(a))) {
                let a = &self.lib.1[i];
                self.acts.push(Act::Album(a.id.clone(), a.name.clone()));
            }
            if self.home.is_empty()
                && let Some(i) = ui::shelf(ui, tr("Novos lançamentos"), &self.new_albums, false, false, |a| (&a.cover, &a.name, album_sub(a)))
            {
                let a = &self.new_albums[i];
                self.acts.push(Act::Album(a.id.clone(), a.name.clone()));
            }
            ui.add_space(24.0);
        });
    }

    /// Grid of shortcuts at the top of Home: liked songs and the library, or recent tracks.
    fn quick_tiles(&mut self, ui: &mut egui::Ui, recent: &[TrackItem]) {
        enum Pick {
            Liked,
            Playlist(usize),
            Album(usize),
            Track(usize),
        }
        let mut picks: Vec<Pick> = vec![];
        if self.logged_in {
            picks.push(Pick::Liked);
            picks.extend((0..self.lib.0.len()).map(Pick::Playlist));
            picks.extend((0..self.lib.1.len()).map(Pick::Album));
        }
        picks.extend((0..recent.len()).map(Pick::Track));
        picks.truncate(8);
        if picks.is_empty() {
            return;
        }
        let cols = ((ui.available_width() + 12.0) / 300.0).floor().clamp(1.0, 4.0) as usize;
        let w = (ui.available_width() - 12.0 * (cols - 1) as f32) / cols as f32;
        for row in picks.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for pick in row {
                    // Each tile takes on its cover's colour (cached per cover, computed once).
                    let cover = match pick {
                        Pick::Liked => None,
                        Pick::Playlist(i) => Some(self.lib.0[*i].thumbnail.clone()),
                        Pick::Album(i) => Some(self.lib.1[*i].cover.clone()),
                        Pick::Track(i) => Some(recent[*i].cover.clone()),
                    };
                    let tint = match cover {
                        Some(c) => self.accent_for(&c, 64.0),
                        None => Some(c().accent),
                    };
                    let (art, title) = match pick {
                        Pick::Liked => (ui::Art::Liked, tr("Músicas curtidas").to_string()),
                        Pick::Playlist(i) => (ui::Art::Cover(&self.lib.0[*i].thumbnail), self.lib.0[*i].name.clone()),
                        Pick::Album(i) => (ui::Art::Cover(&self.lib.1[*i].cover), self.lib.1[*i].name.clone()),
                        Pick::Track(i) => (ui::Art::Cover(&recent[*i].cover), recent[*i].name.clone()),
                    };
                    let (open, play) = ui::quick_tile(ui, art, &title, w, tint);
                    if !(open || play) {
                        continue;
                    }
                    self.acts.push(match pick {
                        Pick::Liked if play => Act::Play(self.liked_tracks.clone(), 0),
                        Pick::Liked => Act::Liked,
                        Pick::Playlist(i) => Act::Playlist(self.lib.0[*i].id.clone(), self.lib.0[*i].name.clone()),
                        Pick::Album(i) => Act::Album(self.lib.1[*i].id.clone(), self.lib.1[*i].name.clone()),
                        Pick::Track(i) => Act::Radio(recent[*i].clone()),
                    });
                }
            });
            ui.add_space(10.0);
        }
    }

    fn results(&mut self, ui: &mut egui::Ui, since: f32) {
        let found = self.found.clone();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if !found.tracks.is_empty() {
                ui::section(ui, tr("Músicas"));
                ui.add_space(8.0);
                let playing = self.current().map(|t| t.id.clone());
                for (i, t) in found.tracks.iter().enumerate() {
                    let rc = self.row_ctx(t, false, true);
                    let click = ui.scope(|ui| {
                        ui.multiply_opacity(ui::cascade(since, i));
                        ui::track_row(ui, t, playing.as_deref() == Some(t.id.as_str()), &rc)
                    });
                    if let Some(cl) = click.inner {
                        // A lone track (search result) starts a radio so playback keeps going.
                        let act = self.row_act(cl, t, Act::Radio(t.clone()), Act::Radio(t.clone()));
                        self.acts.extend(act);
                    }
                }
            }
            let sub = |a: &rustypipe::model::ArtistItem| a.subscriber_count.map_or(tr("Artista").into(), |n| tf!("{} inscritos", compact_num(n)));
            if let Some(i) = ui::shelf(ui, tr("Artistas"), &found.artists, false, true, |a| (&a.avatar, &a.name, sub(a))) {
                self.acts.push(Act::Artist(found.artists[i].id.clone()));
            }
            if let Some(i) = ui::shelf(ui, tr("Álbuns"), &found.albums, false, false, |a| (&a.cover, &a.name, album_sub(a))) {
                let a = &found.albums[i];
                self.acts.push(Act::Album(a.id.clone(), a.name.clone()));
            }
            if let Some(i) = ui::shelf(ui, tr("Playlists"), &found.playlists, true, false, |p| (&p.thumbnail, &p.name, playlist_sub(p))) {
                let pl = &found.playlists[i];
                self.acts.push(Act::Playlist(pl.id.clone(), pl.name.clone()));
            }
            ui.add_space(24.0);
        });
    }

    fn list_page(&mut self, ui: &mut egui::Ui, since: f32) {
        let l = self.list.clone();
        let owned = l.kind == ListKind::Playlist && self.owns(&l.id);
        ui.add_space(16.0);
        let cover = if ui.available_width() < 720.0 { 140.0 } else { 200.0 };
        ui.horizontal(|ui| {
            let (cr, resp) = ui.allocate_exact_size(vec2(cover, cover), egui::Sense::hover());
            ui::tilt_cover(ui, &resp, cr, &l.cover, 8, 0.18);
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.set_min_height(cover);
                ui.add_space(if cover < 200.0 { 8.0 } else { 36.0 });
                weak(ui, match l.kind {
                    ListKind::Album => tr("Álbum"),
                    ListKind::Playlist => tr("Playlist"),
                    ListKind::Liked => tr("Playlist automática"),
                });
                ui.label(ui::line(ui, &l.title, ui::bold(32.0), c().text, ui.available_width()));
                let secs: u64 = l.tracks.iter().filter_map(|t| t.duration).map(u64::from).sum();
                let mins = secs / 60;
                let len = if mins >= 60 { format!("{} h {} min", mins / 60, mins % 60) } else { format!("{mins} min") };
                weak(ui, tf!("{} faixas • {}", l.tracks.len(), len));
                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui| {
                    if ui::pill(ui, Some(Icon::Play), tr("Tocar"), true).clicked() {
                        self.acts.push(Act::Play(l.tracks.clone(), 0));
                    }
                    if ui::pill(ui, Some(Icon::Shuffle), tr("Aleatório"), false).clicked() && !l.tracks.is_empty() {
                        self.acts.push(Act::Play(crate::shuffled(l.tracks.clone()), 0));
                    }
                    if let Some(rid) = &l.radio_id
                        && ui::pill(ui, Some(Icon::Radio), tr("Rádio"), false).clicked() {
                            self.acts.push(Act::RadioOf(format!("RDAMPL{rid}")));
                        }
                    if ui::pill(ui, None, tr("Copiar link"), false).clicked() {
                        let link = match l.kind {
                            ListKind::Album => format!("https://music.youtube.com/browse/{}", l.id),
                            ListKind::Liked => "https://music.youtube.com/playlist?list=LM".into(),
                            ListKind::Playlist => format!("https://music.youtube.com/playlist?list={}", l.id),
                        };
                        ui.ctx().copy_text(link);
                        self.status = tr("Link copiado.").into();
                    }
                    if owned {
                        if !self.confirm_delete {
                            if ui::pill(ui, None, tr("Excluir"), false).clicked() {
                                self.confirm_delete = true;
                            }
                        } else if ui::pill(ui, None, tr("Confirmar exclusão"), false).clicked() {
                            self.acts.push(Act::DeletePlaylist(l.id.clone()));
                        }
                    }
                });
            });
        });
        ui.add_space(20.0);
        let opts = TrackListOpts { id: "list", removable: owned, movable: self.can_reorder(), show_album: l.kind != ListKind::Album, queue_offset: None, solo: false };
        self.track_list(ui, &l.tracks, since, opts);
    }

    fn artist_page(&mut self, ui: &mut egui::Ui) {
        let Some(a) = self.artist.clone() else { return };
        let p = c();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(12.0);
            let w = ui.available_width();
            let (banner, _) = ui.allocate_exact_size(vec2(w, 300.0), egui::Sense::hover());
            ui.painter().rect_filled(banner, 8.0, p.bar);
            ui::paint_banner(ui, &a.header_image, banner);
            let text = banner.shrink(24.0);
            ui.scope_builder(egui::UiBuilder::new().max_rect(text).layout(egui::Layout::bottom_up(egui::Align::Min)), |ui| {
                ui.horizontal(|ui| {
                    if ui::pill(ui, Some(Icon::Play), tr("Tocar"), true).clicked() {
                        self.acts.push(Act::Play(a.tracks.clone(), 0));
                    }
                    if ui::pill(ui, Some(Icon::Shuffle), tr("Aleatório"), false).clicked() && !a.tracks.is_empty() {
                        self.acts.push(Act::Play(crate::shuffled(a.tracks.clone()), 0));
                    }
                    if let Some(rid) = &a.radio_id
                        && ui::pill(ui, Some(Icon::Radio), tr("Rádio"), false).clicked() {
                            self.acts.push(Act::RadioOf(rid.clone()));
                        }
                    if ui::pill(ui, None, tr("Copiar link"), false).clicked() {
                        ui.ctx().copy_text(format!("https://music.youtube.com/channel/{}", a.id));
                        self.status = tr("Link copiado.").into();
                    }
                });
                ui.add_space(8.0);
                if let Some(n) = a.subscriber_count {
                    weak(ui, tf!("{} inscritos", compact_num(n)));
                }
                ui.label(ui::line(ui, &a.name, ui::bold(44.0), p.text, text.width()));
            });
            if let Some(d) = &a.description {
                ui.add_space(12.0);
                let short: String = d.chars().take(320).collect();
                let more = if d.chars().count() > 320 { "…" } else { "" };
                ui.label(RichText::new(format!("{short}{more}")).color(p.text2));
            }
            if !a.tracks.is_empty() {
                ui::section(ui, tr("Músicas"));
                ui.add_space(8.0);
                let playing = self.current().map(|t| t.id.clone());
                for (i, t) in a.tracks.iter().enumerate() {
                    let rc = self.row_ctx(t, false, true);
                    if let Some(cl) = ui::track_row(ui, t, playing.as_deref() == Some(t.id.as_str()), &rc) {
                        let act = self.row_act(cl, t, Act::Play(a.tracks.clone(), i), Act::Radio(t.clone()));
                        self.acts.extend(act);
                    }
                }
                if let Some(pid) = &a.tracks_playlist_id {
                    ui.add_space(8.0);
                    if ui::pill(ui, None, tr("Ver todas as músicas"), false).clicked() {
                        self.acts.push(Act::Playlist(pid.clone(), tf!("Músicas de {}", a.name)));
                    }
                }
            }
            if let Some(i) = ui::shelf(ui, tr("Álbuns e singles"), &a.albums, false, false, |al| (&al.cover, &al.name, al.year.map_or(String::new(), |y| y.to_string()))) {
                let al = &a.albums[i];
                self.acts.push(Act::Album(al.id.clone(), al.name.clone()));
            }
            if let Some(i) = ui::shelf(ui, tr("Playlists"), &a.playlists, true, false, |pl| (&pl.thumbnail, &pl.name, playlist_sub(pl))) {
                let pl = &a.playlists[i];
                self.acts.push(Act::Playlist(pl.id.clone(), pl.name.clone()));
            }
            let sub = |s: &rustypipe::model::ArtistItem| s.subscriber_count.map_or(tr("Artista").into(), |n| tf!("{} inscritos", compact_num(n)));
            if let Some(i) = ui::shelf(ui, tr("Artistas semelhantes"), &a.similar_artists, false, true, |s| (&s.avatar, &s.name, sub(s))) {
                self.acts.push(Act::Artist(a.similar_artists[i].id.clone()));
            }
            ui.add_space(24.0);
        });
    }

    /// What you've listened to most, from the local history (its last 500 plays at most).
    fn summary(&mut self, ui: &mut egui::Ui, since: f32) {
        let p = c();
        let all_id = ui.id().with("summary-all");
        let mut all: bool = ui.data(|d| d.get_temp(all_id)).unwrap_or(false);
        ui.horizontal(|ui| {
            for (v, label) in [(false, tr("Últimos 30 dias")), (true, tr("Todo o histórico"))] {
                if ui::chip(ui, label, all == v).clicked() {
                    all = v;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(all_id, all));
        ui.add_space(12.0);
        let since_secs = crate::now_secs().saturating_sub(30 * 86_400);
        let plays: Vec<&crate::settings::Played> = self.history.iter().filter(|h| all || h.at >= since_secs).collect();
        if plays.is_empty() {
            weak(ui, if self.s.history { tr("Nada tocado nesse período.") } else { tr("O histórico está desligado nas configurações.") });
            return;
        }
        let mins: u64 = plays.iter().filter_map(|h| h.track.duration).map(u64::from).sum::<u64>() / 60;
        let time = if mins >= 60 { format!("{} h {} min", mins / 60, mins % 60) } else { format!("{mins} min") };
        ui.label(RichText::new(tf!("{} músicas tocadas • {} ouvindo", plays.len(), time)).font(ui::bold(22.0)).color(p.text));
        ui.add_space(16.0);
        // Most played artists (by the first artist of each track) and tracks, most first; ties keep
        // the most recent first.
        let mut artists: Vec<(String, usize)> = vec![];
        let mut tracks: Vec<(TrackItem, usize)> = vec![];
        for h in &plays {
            if let Some(a) = h.track.artists.first() {
                match artists.iter_mut().find(|(n, _)| *n == a.name) {
                    Some(e) => e.1 += 1,
                    None => artists.push((a.name.clone(), 1)),
                }
            }
            match tracks.iter_mut().find(|(t, _)| t.id == h.track.id) {
                Some(e) => e.1 += 1,
                None => tracks.push((h.track.clone(), 1)),
            }
        }
        artists.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        tracks.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        ui.label(RichText::new(tr("Artistas mais ouvidos")).font(ui::bold(17.0)).color(p.text));
        ui.add_space(6.0);
        for (i, (name, n)) in artists.iter().take(5).enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}", i + 1)).font(ui::bold(15.0)).color(p.accent));
                ui.label(RichText::new(name).font(ui::medium(15.0)).color(p.text));
                weak(ui, if *n == 1 { tf!("{} vez", n) } else { tf!("{} vezes", n) });
            });
        }
        ui.add_space(16.0);
        ui.label(RichText::new(tr("Músicas mais ouvidas")).font(ui::bold(17.0)).color(p.text));
        ui.add_space(6.0);
        let top: Vec<TrackItem> = tracks.into_iter().take(10).map(|(t, _)| t).collect();
        let opts = TrackListOpts { id: "summary", removable: false, movable: false, show_album: true, queue_offset: None, solo: false };
        self.track_list(ui, &top, since, opts);
    }

    fn queue_page(&mut self, ui: &mut egui::Ui, since: f32) {
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            for (i, label) in [tr("A seguir"), tr("Recentes"), tr("Seu resumo")].into_iter().enumerate() {
                if ui::pill(ui, None, label, self.queue_tab == i).clicked() {
                    self.queue_tab = i;
                }
            }
        });
        ui.add_space(12.0);
        if self.queue_tab == 2 {
            return self.summary(ui, since);
        }
        if self.queue_tab == 1 {
            if self.history.is_empty() {
                weak(ui, if self.s.history { tr("Nada tocado ainda.") } else { tr("O histórico está desligado nas configurações.") });
                return;
            }
            let tracks: Vec<TrackItem> = self.history.iter().map(|h| h.track.clone()).collect();
            let opts = TrackListOpts { id: "recent", removable: false, movable: false, show_album: true, queue_offset: None, solo: true };
            return self.track_list(ui, &tracks, since, opts);
        }
        let start = self.pl.pos.max(0) as usize;
        let upcoming: Vec<TrackItem> = self.queue.iter().skip(start).cloned().collect();
        ui.horizontal(|ui| {
            let note = if self.radio_on { tr(" • rádio") } else { "" };
            weak(ui, tf!("{} faixas{}", upcoming.len(), note));
            ui.add_space(12.0);
            if self.logged_in && !upcoming.is_empty() {
                self.new_playlist_input(ui, true);
            }
            if upcoming.len() > 1 && ui::pill(ui, None, tr("Limpar"), false).clicked() {
                self.acts.push(Act::ClearQueue);
            }
        });
        ui.add_space(8.0);
        let opts = TrackListOpts { id: "queue", removable: true, movable: true, show_album: true, queue_offset: Some(start), solo: false };
        self.track_list(ui, &upcoming, since, opts);
    }

    /// Player-bar button / L: side panel on and off; from the full-window lyrics, back to the panel.
    fn toggle_lyrics(&mut self) -> Option<Act> {
        if self.view == View::Lyrics {
            return Some(Act::Back);
        }
        self.lyrics_panel = !self.lyrics_panel;
        if self.lyrics_panel {
            self.load_lyrics();
        }
        None
    }

    /// "−0.5 s / +0.5 s" for synced lyrics that run ahead of or behind the song, per track.
    fn lyrics_nudge_buttons(&mut self, ui: &mut egui::Ui, col: Color32) {
        let synced = self.lyrics.as_ref().is_some_and(|(_, l)| l.as_ref().is_some_and(|l| !l.synced.is_empty()));
        let Some(id) = self.current().map(|t| t.id.clone()).filter(|_| synced) else { return };
        let nudge = self.lyrics_nudge();
        let mut set = |v: f64| self.lyrics_offset = (id.clone(), (v * 10.0).round() / 10.0);
        if ui.add(egui::Button::new(RichText::new("+").color(col)).small()).on_hover_text(tr("Letra atrasada: adiantar 0,5 s")).clicked() {
            set(nudge + 0.5);
        }
        if nudge != 0.0 {
            let label = format!("{nudge:+.1} s").replace('.', if crate::i18n::english() { "." } else { "," });
            if ui.add(egui::Button::new(RichText::new(label).color(col)).small().frame(false)).on_hover_text(tr("Voltar ao tempo original")).clicked() {
                set(0.0);
            }
        }
        if ui.add(egui::Button::new(RichText::new("−").color(col)).small()).on_hover_text(tr("Letra adiantada: atrasar 0,5 s")).clicked() {
            set(nudge - 0.5);
        }
    }

    fn lyrics_side(&mut self, ui: &mut egui::Ui) {
        let p = c();
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Letra")).font(ui::bold(20.0)).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::icon_button(ui, Icon::Close, 30.0).on_hover_text(tr("Fechar")).clicked() {
                    self.lyrics_panel = false;
                }
                if ui::icon_button(ui, Icon::Expand, 30.0).on_hover_text(tr("Tela cheia")).clicked() {
                    self.acts.push(Act::Go(View::Lyrics));
                }
                self.lyrics_nudge_buttons(ui, p.text2);
            });
        });
        ui.add_space(8.0);
        self.lyric_lines(ui, 19.0, p.accent);
    }

    /// Full window: blurred-looking art wash, cover on the left, big synced lines on the right.
    fn lyrics_full(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let t = self.current().cloned();
        if let Some(col) = t.as_ref().and_then(|t| self.accent_for(&t.cover, 280.0)) {
            ui::gradient(ui, full, col.gamma_multiply(0.30), Color32::TRANSPARENT);
        }
        let head = Rect::from_min_size(full.min + vec2(28.0, 10.0), vec2(full.width() - 56.0, 34.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.label(RichText::new(tr("Letra")).font(ui::bold(18.0)).color(Color32::WHITE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::icon_button(ui, Icon::Collapse, 30.0).on_hover_text(tr("Sair da tela cheia (Esc)")).clicked() {
                    self.lyrics_panel = true;
                    self.acts.push(Act::Back);
                }
                self.lyrics_nudge_buttons(ui, Color32::from_gray(200));
            });
        });
        let body = Rect::from_min_max(pos2(full.left() + 48.0, head.bottom() + 16.0), pos2(full.right() - 48.0, full.bottom()));
        let wide = body.width() > 820.0;
        let cover_side = if wide { (body.width() * 0.32).min(body.height() - 140.0).clamp(200.0, 520.0) } else { 0.0 };
        let gap = if wide { 64.0 } else { 0.0 };
        // Cover + lyrics, centred as a pair.
        let lyrics_w = (body.width() - cover_side - gap).min(760.0);
        let left = body.center().x - (cover_side + gap + lyrics_w) / 2.0;
        if let (true, Some(t)) = (wide, &t) {
            let cr = Rect::from_min_size(pos2(left, body.center().y - cover_side / 2.0 - 40.0), vec2(cover_side, cover_side));
            let resp = ui.interact(cr, ui.id().with("lyr-cover"), egui::Sense::hover());
            ui::tilt_cover(ui, &resp, cr, &t.cover, 10, 0.12);
            let name = ui::line(ui, &t.name, ui::bold(22.0), Color32::WHITE, cover_side);
            let by = ui::line(ui, &ui::artists(t), ui::regular(15.0), Color32::from_gray(190), cover_side);
            ui.painter().galley(pos2(cr.left(), cr.bottom() + 16.0), name.clone(), Color32::WHITE);
            ui.painter().galley(pos2(cr.left(), cr.bottom() + 22.0 + name.size().y), by, Color32::WHITE);
        }
        let lr = Rect::from_min_max(pos2(left + cover_side + gap, body.top()), pos2(left + cover_side + gap + lyrics_w, body.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(lr).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            self.lyric_lines(ui, 30.0, Color32::WHITE);
        });
    }

    /// The lines themselves; synced ones follow the song, dim once sung, and seek when clicked.
    fn lyric_lines(&mut self, ui: &mut egui::Ui, size: f32, hi: Color32) {
        let p = c();
        let Some(t) = self.current().cloned() else {
            weak(ui, tr("Toque algo para ver a letra."));
            return;
        };
        // Cheap no-op when the current track's lyrics are already loaded or loading.
        self.load_lyrics();
        let Some((_, Some(ly))) = &self.lyrics else {
            weak(ui, tr("Carregando letra…"));
            return;
        };
        if ly.is_empty() {
            weak(ui, tf!("Sem letra para \"{}\".", t.name));
            return;
        }
        let ly = ly.clone();
        let full = self.view == View::Lyrics;
        let (dim, past) = if full { (Color32::from_gray(150), Color32::from_gray(110)) } else { (p.text2, p.text2.gamma_multiply(0.6)) };
        let nudge = self.lyrics_nudge();
        let now = self.mpv.get_property::<f64>("time-pos").unwrap_or(self.pl.time) + nudge;
        let cur = ly.line_at(now + 0.15);
        if !ly.synced.is_empty() && !self.pl.paused {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
        }
        let id = ui.id().with("lyr-follow");
        let last: Option<usize> = ui.data(|d| d.get_temp(id)).flatten();
        let moved = last != cur;
        ui.data_mut(|d| d.insert_temp(id, cur));
        let gap = size * 0.9;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(size);
            if ly.synced.is_empty() {
                for line in ly.plain.lines() {
                    ui.label(RichText::new(line).font(ui::bold(size)).color(if full { Color32::WHITE } else { p.text }));
                }
            } else {
                for (i, (at, line)) in ly.synced.iter().enumerate() {
                    let text = if line.trim().is_empty() { "♪" } else { line.as_str() };
                    let col = match cur {
                        Some(c) if c == i => hi,
                        Some(c) if i < c => past,
                        _ => dim,
                    };
                    // Karaoke: on the line being sung, each word lights up as it's sung.
                    let words = ly.words.get(i).filter(|w| self.s.lyrics_words && Some(i) == cur && !w.is_empty());
                    let label = match words {
                        Some(words) => {
                            let mut job = egui::text::LayoutJob::default();
                            for (t0, t1, w) in words {
                                let k = if now >= *t1 { 1.0 } else if now <= *t0 { 0.0 } else { ((now - t0) / (t1 - t0).max(0.01)) as f32 };
                                job.append(w, 0.0, egui::TextFormat::simple(ui::bold(size), dim.lerp_to_gamma(hi, k)));
                            }
                            job.wrap.max_width = ui.available_width();
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
                            egui::Label::new(job)
                        }
                        None => egui::Label::new(RichText::new(text).font(ui::bold(size)).color(col)),
                    };
                    let resp = ui.add(label.wrap().sense(egui::Sense::click()));
                    if Some(i) == cur && moved {
                        ui.scroll_to_rect(resp.rect, Some(egui::Align::Center));
                    }
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr("Ir para este trecho")).clicked() {
                        self.mpv.command("seek", &[&(at - nudge).max(0.0).to_string(), "absolute"]).ok();
                    }
                    ui.add_space(gap * 0.5);
                }
            }
            ui.add_space(24.0);
            if !ly.source.is_empty() {
                weak(ui, &ly.source);
            }
            ui.add_space(size * 4.0);
        });
    }

    fn video_page(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let p = c();
        let track = self.current().cloned();
        ui.add_space(12.0);
        let avail = ui.available_rect_before_wrap();
        let info_h = 64.0;
        let area = Rect::from_min_max(avail.min, pos2(avail.right(), avail.bottom() - info_h - 12.0));
        ui.painter().rect_filled(area, 12.0, Color32::BLACK);
        let gray = Color32::from_gray(170);
        match &track {
            None => {
                ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, tr("Toque algo para ver o vídeo."), ui::regular(15.0), gray);
            }
            Some(_) if self.gpa.is_none() => {
                ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, tr("Vídeo indisponível neste sistema (requer OpenGL)."), ui::regular(15.0), gray);
            }
            // The music keeps playing while the picture loads: the cover stands in until frames come.
            Some(t) if t.track_type == rustypipe::model::TrackType::Video => {
                if self.mpv.get_property::<i64>("video-params/w").is_ok_and(|w| w > 0) {
                    self.paint_video(ui, frame, area.shrink(1.0));
                } else {
                    self.ensure_vis();
                    let side = (area.height() * 0.55).min(area.width() * 0.4);
                    let cover = Rect::from_center_size(area.center() - vec2(0.0, 24.0), vec2(side, side));
                    ui::paint_cover(ui, &t.cover, cover, 8);
                    ui.painter().text(pos2(area.center().x, cover.bottom() + 22.0), egui::Align2::CENTER_CENTER, tr("Carregando o vídeo…"), ui::regular(14.0), gray);
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                }
            }
            Some(t) => {
                // Album tracks have no real video on YouTube Music (only a still of the cover).
                let side = (area.height() * 0.55).min(area.width() * 0.4);
                let cover = Rect::from_center_size(area.center() - vec2(0.0, 24.0), vec2(side, side));
                ui::paint_cover(ui, &t.cover, cover, 8);
                ui.painter().text(pos2(area.center().x, cover.bottom() + 22.0), egui::Align2::CENTER_CENTER, tr("Esta faixa é só áudio."), ui::regular(14.0), gray);
            }
        }
        // Title row under the video.
        let info = Rect::from_min_max(pos2(avail.left(), area.bottom() + 12.0), pos2(avail.right(), avail.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(info).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            if let Some(t) = &track {
                ui.vertical(|ui| {
                    ui.label(ui::line(ui, &t.name, ui::bold(18.0), p.text, info.width() - 360.0));
                    ui.label(ui::line(ui, &ui::artists(t), ui::regular(14.0), p.text2, info.width() - 360.0));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if t.track_type == rustypipe::model::TrackType::Video {
                        if ui::pill(ui, None, tr("Tela cheia (F)"), false).clicked() {
                            self.vis_full = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                        }
                    } else if ui::pill(ui, Some(Icon::Video), tr("Procurar videoclipe"), true).clicked() {
                        self.acts.push(Act::FindVideo);
                    }
                });
            }
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        let p = c();
        // Card titles are translated, so the selected tab is compared in the same language.
        let tab = tr(SETTINGS_TABS.get(self.settings_tab).map_or(SETTINGS_TABS[0].1, |t| t.1));
        // Only the selected category is drawn; the others are skipped.
        let card = |ui: &mut egui::Ui, title: &str, body: &mut dyn FnMut(&mut egui::Ui)| {
            if title != tab {
                return;
            }
            heading(ui, title, 24.0);
            ui.add_space(12.0);
            egui::Frame::NONE.fill(p.bar).corner_radius(12).inner_margin(egui::Margin::symmetric(22, 16)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                body(ui);
            });
        };
        let mut acts = vec![];
        let mut open_help = false;
        let logged_in = self.logged_in;
        let s = &mut self.s;
        let mpv = self.mpv;
        let mut new_tab = None;
        // Wide: categories in a column on the left. Narrow: a row of chips above the content.
        let wide = ui.available_width() >= 760.0;
        let content_w = if wide { (ui.available_width() - 210.0 - 28.0 - 24.0).clamp(320.0, 720.0) } else { ui.available_width() - 8.0 };
        ui.add_space(8.0);
        heading(ui, tr("Configurações"), 28.0);
        ui.add_space(12.0);
        if !wide {
            ui.horizontal_wrapped(|ui| {
                for (i, (_, label)) in SETTINGS_TABS.iter().enumerate() {
                    if ui::chip(ui, tr(label), tr(label) == tab).clicked() {
                        new_tab = Some(i);
                    }
                }
            });
            ui.add_space(12.0);
        }
        ui.horizontal_top(|ui| {
        if wide {
            ui.vertical(|ui| {
                ui.set_width(210.0);
                for (i, (icon, label)) in SETTINGS_TABS.iter().enumerate() {
                    if ui::nav_item(ui, *icon, tr(label), tr(label) == tab).clicked() {
                        new_tab = Some(i);
                    }
                }
            });
            ui.add_space(28.0);
        }
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // The parent row is horizontal; without this the heading and card sit side by side.
        ui.vertical(|ui| {
            ui.set_width(content_w);

            card(ui, tr("Reprodução"), &mut |ui| {
                choice(ui, tr("Qualidade do áudio"), &mut s.quality, &[(Quality::High, tr("Alta")), (Quality::Normal, tr("Normal")), (Quality::Low, tr("Economia de dados"))]);
                let devices: Vec<(String, String)> = mpv
                    .get_property::<String>("audio-device-list")
                    .ok()
                    .and_then(|j| serde_json::from_str::<Vec<serde_json::Value>>(&j).ok())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|d| {
                        let name = d["name"].as_str()?.to_string();
                        let desc = if name == "auto" { tr("Automático (padrão do sistema)").to_string() } else { d["description"].as_str()?.to_string() };
                        Some((name, desc))
                    })
                    .collect();
                let current = devices.iter().find(|(n, _)| *n == s.audio_device).map_or(s.audio_device.clone(), |(_, d)| d.clone());
                row(ui, tr("Saída de áudio"), |ui| {
                    egui::ComboBox::from_id_salt("dev").selected_text(current).width(260.0).show_ui(ui, |ui| {
                        for (name, desc) in &devices {
                            ui.selectable_value(&mut s.audio_device, name.clone(), desc);
                        }
                    });
                });
                ui::switch(ui, &mut s.gapless, tr("Sem pausa entre faixas"), tr("Emenda uma música na outra (gapless)"));
                choice(
                    ui,
                    tr("Transição suave (crossfade)"),
                    &mut s.crossfade_secs,
                    &[(0, tr("Desligada")), (2, tr("2 segundos")), (4, tr("4 segundos")), (6, tr("6 segundos")), (8, tr("8 segundos")), (10, tr("10 segundos")), (12, tr("12 segundos"))],
                );
                ui::switch(ui, &mut s.normalize, tr("Normalizar volume"), tr("Deixa todas as músicas com volume parecido"));
                ui::switch(ui, &mut s.autoplay, tr("Reprodução automática"), tr("Continua com músicas parecidas quando a fila acaba"));
                ui::switch(ui, &mut s.mono, tr("Áudio mono"), tr("Os dois lados tocam o mesmo som"));
                choice(ui, tr("Velocidade"), &mut s.speed, &[(0.75, tr("0,75×")), (1.0, tr("Normal")), (1.25, tr("1,25×")), (1.5, tr("1,5×")), (1.75, tr("1,75×")), (2.0, "2×")]);
                row(ui, tr("Balanço"), |ui| {
                    ui.add(egui::Slider::new(&mut s.balance, -1.0..=1.0).show_value(false).trailing_fill(false));
                    if s.balance != 0.0 && ui.small_button(tr("Centralizar")).clicked() {
                        s.balance = 0.0;
                    }
                });
                choice(ui, tr("Buffer de rede"), &mut s.buffer_mb, &[(32, "32 MB"), (64, "64 MB"), (128, "128 MB"), (256, "256 MB")]);
            });

            card(ui, tr("Equalizador"), &mut |ui| {
                ui::switch(ui, &mut s.eq_on, tr("Ativar equalizador"), tr("Ajusta graves, médios e agudos de tudo que toca"));
                ui.add_enabled_ui(s.eq_on, |ui| {
                    ui::switch(ui, &mut s.eq_loudness, tr("Loudness adaptativo"), tr("Com volume baixo o ouvido perde graves e agudos; eles voltam aos poucos conforme você abaixa"));
                });
                ui.add_space(8.0);
                ui.add_enabled_ui(s.eq_on, |ui| {
                    if !s.eq_on {
                        ui.multiply_opacity(0.45);
                    }
                    ui.horizontal_wrapped(|ui| {
                        for (name, bands) in EQ_PRESETS {
                            let selected = s.eq_bands_db == *bands;
                            if ui::chip(ui, tr(name), selected).clicked() {
                                s.eq_bands_db = *bands;
                            }
                        }
                    });
                    ui.add_space(14.0);
                    eq_editor(ui, &mut s.eq_preamp_db, &mut s.eq_bands_db);
                });
            });

            card(ui, tr("Aparência"), &mut |ui| {
                // Language names in their own language, so anyone can find theirs.
                let langs = [("auto", tr("Automático (sistema)")), ("pt", "Português (Brasil)"), ("en", "English")];
                let current = langs.iter().find(|(k, _)| *k == s.language).map_or("English", |(_, n)| n);
                row(ui, tr("Idioma"), |ui| {
                    egui::ComboBox::from_id_salt("language").selected_text(current).width(220.0).show_ui(ui, |ui| {
                        for (k, name) in langs {
                            ui.selectable_value(&mut s.language, k.to_string(), name);
                        }
                    });
                });
                let mut themes: Vec<(String, String)> = vec![("system".into(), tr("Seguir o sistema").into()), ("dark".into(), tr("Escuro").into()), ("light".into(), tr("Claro").into())];
                themes.extend(theme::custom_themes().into_iter().map(|f| (f.clone(), theme::display_name(&f))));
                let current = themes.iter().find(|(k, _)| *k == s.theme).map_or(s.theme.clone(), |(_, n)| n.clone());
                row(ui, tr("Tema"), |ui| {
                    egui::ComboBox::from_id_salt("theme").selected_text(current).width(220.0).show_ui(ui, |ui| {
                        for (k, name) in &themes {
                            ui.selectable_value(&mut s.theme, k.clone(), name);
                        }
                    });
                    if ui.small_button(tr("Abrir pasta de temas")).clicked() {
                        open_path(&theme::themes_dir());
                    }
                });
                ui::switch(ui, &mut s.accent_from_art, tr("Cores da capa"), tr("Pinta as páginas com a cor da capa do álbum"));
                ui::switch(ui, &mut s.effects_3d, tr("Efeito 3D nas capas"), tr("Capas inclinam em direção ao cursor"));
                if s.effects_3d {
                    row(ui, tr("Intensidade do 3D"), |ui| {
                        ui.add(egui::Slider::new(&mut s.tilt, 0.3..=2.0).show_value(false));
                    });
                }
                ui::switch(ui, &mut s.reduce_motion, tr("Reduzir animações"), tr("Desliga transições e animações de hover"));
                choice(ui, tr("Estilo da letra"), &mut s.lyrics_words, &[(false, tr("Por linha")), (true, tr("Palavra por palavra (karaokê)"))]);
                ui::switch(ui, &mut s.tracklist_compact, tr("Lista de faixas compacta"), tr("Uma linha por faixa, sem capas"));
                ui::switch(ui, &mut s.sidebar_compact, tr("Barra lateral compacta"), tr("Só os nomes, sem subtítulo"));
                choice(ui, tr("Tamanho da interface"), &mut s.zoom, &[(0.8, "80%"), (0.9, "90%"), (1.0, "100%"), (1.1, "110%"), (1.25, "125%"), (1.5, "150%")]);
            });

            card(ui, tr("Janela"), &mut |ui| {
                ui::switch(ui, &mut s.close_to_tray, tr("Fechar para a bandeja"), tr("A música continua tocando com a janela fechada"));
                ui::switch(ui, &mut s.mini_on_top, tr("Mini player sempre no topo"), "");
                ui::switch(ui, &mut s.start_mini, tr("Abrir no mini player"), "");
                ui::switch(ui, &mut s.autostart_tray, tr("Iniciar com o computador, na bandeja"), tr("Toca e responde aos controles sem abrir janela: bem mais leve até você clicar no ícone"));
                #[cfg(target_os = "linux")]
                ui::switch(ui, &mut s.notify_track, tr("Avisar quando a música mudar"), tr("Notificação do sistema, só com a janela escondida ou em segundo plano"));
            });

            card(ui, tr("Integrações"), &mut |ui| {
                row(ui, "ListenBrainz", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.listenbrainz_token).password(true).hint_text(tr("token de usuário")).desired_width(260.0));
                });
                ui.horizontal_wrapped(|ui| {
                    weak(ui, tr("Registra o que você ouve no seu perfil. O token fica em"));
                    ui.hyperlink_to("listenbrainz.org/settings", "https://listenbrainz.org/settings/");
                });
            });

            card(ui, "Winamp", &mut |ui| {
                let installed = crate::winamp::list_skins();
                let has_classic = installed.iter().any(|k| k == crate::winamp::CLASSIC_NAME);
                let mut skins: Vec<(String, String)> = vec![(String::new(), tr("Nenhuma (player compacto do YtmFast)").into())];
                if has_classic {
                    skins.push((crate::winamp::THEMED.into(), tr("Clássica nas cores do tema").into()));
                }
                skins.extend(installed.iter().map(|f| (f.clone(), f.trim_end_matches(".wsz").to_string())));
                let current = skins.iter().find(|(k, _)| *k == s.winamp_skin).map_or(s.winamp_skin.clone(), |(_, n)| n.clone());
                row(ui, tr("Skin do mini player"), |ui| {
                    egui::ComboBox::from_id_salt("skin").selected_text(current).width(240.0).show_ui(ui, |ui| {
                        for (k, name) in &skins {
                            ui.selectable_value(&mut s.winamp_skin, k.clone(), name);
                        }
                    });
                });
                if s.winamp_skin.is_empty() {
                    choice(ui, tr("Estilo do player compacto"), &mut s.deck_style, &[
                        (crate::settings::DeckStyle::Modern, tr("Moderno")),
                        (crate::settings::DeckStyle::Classic, tr("Clássico")),
                        (crate::settings::DeckStyle::Lcd, tr("LCD")),
                    ]);
                }
                if installed.len() > 1 {
                    ui::switch(ui, &mut s.random_skin, tr("Skin aleatória"), tr("Sorteia outra skin cada vez que o mini player abre"));
                }
                if installed.is_empty() {
                    weak(ui, tr("Nenhuma skin instalada ainda. Baixe a clássica ou coloque arquivos .wsz na pasta de skins."));
                }
                // Skins to download (Winamp's base skin and the ones Webamp's demo offers).
                let missing: Vec<(usize, &str)> = crate::winamp::GALLERY.iter().enumerate().filter(|(_, (f, _))| !installed.iter().any(|k| k == f)).map(|(i, (f, _))| (i, f.trim_end_matches(".wsz"))).collect();
                if !missing.is_empty() {
                    ui.add_space(6.0);
                    weak(ui, tr("Baixar skins:"));
                    ui.horizontal_wrapped(|ui| {
                        for (i, name) in missing {
                            if ui::pill(ui, Some(Icon::Plus), name, i == 0).clicked() {
                                acts.push(Act::DownloadSkin(i));
                            }
                        }
                    });
                }
                if !s.winamp_skin.is_empty() || s.deck_style != crate::settings::DeckStyle::Modern {
                    ui.add_space(6.0);
                    if ui::pill(ui, Some(Icon::Mini), tr("Abrir mini player"), false).clicked() {
                        acts.push(Act::Mini(true));
                    }
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button(tr("Abrir pasta de skins")).clicked() {
                        open_path(&crate::winamp::skins_dir());
                    }
                    ui.hyperlink_to(tr("Mais skins no Winamp Skin Museum"), "https://skins.webamp.org");
                });
                choice(ui, tr("Escala da skin"), &mut s.skin_scale, &[(1, "1×"), (2, "2×"), (3, "3×"), (4, "4×")]);
            });

            card(ui, tr("Conta"), &mut |ui| {
                if logged_in {
                    weak(ui, tr("Conectado ao YouTube Music."));
                    ui.add_space(6.0);
                    if ui::pill(ui, None, tr("Sair da conta"), false).clicked() {
                        acts.push(Act::Logout);
                    }
                } else {
                    weak(ui, tr("Sem login: busca e reprodução funcionam, biblioteca não."));
                    ui.add_space(6.0);
                    if ui::pill(ui, Some(Icon::Person), tr("Entrar"), true).clicked() {
                        acts.push(Act::Go(View::Login));
                    }
                }
            });

            card(ui, tr("Rede"), &mut |ui| {
                row(ui, tr("Proxy"), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut s.proxy).hint_text(tr("vazio = do sistema · http://host:porta")).desired_width(280.0));
                });
                weak(ui, tr("A reprodução usa o novo proxy na hora; a busca, depois de reiniciar o app."));
            });

            card(ui, tr("Armazenamento"), &mut |ui| {
                ui::switch(ui, &mut s.history, tr("Guardar histórico"), tr("Fica só neste computador, nunca é enviado"));
                for (label, path) in [(tr("Configurações"), config_dir()), (tr("Sessão e histórico"), state_dir()), (tr("Cache"), cache_dir())] {
                    row(ui, label, |ui| {
                        if ui.link(path.display().to_string()).clicked() {
                            open_path(&path);
                        }
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui::pill(ui, None, tr("Limpar cache"), false).clicked() {
                        acts.push(Act::ClearCache);
                    }
                    if ui::pill(ui, None, tr("Limpar histórico"), false).clicked() {
                        acts.push(Act::ClearHistory);
                    }
                });
            });

            card(ui, tr("Sobre"), &mut |ui| {
                let ytdlp = if self.ytdlp_version.is_empty() { tr("não encontrado") } else { &self.ytdlp_version };
                weak(ui, format!("YtmFast {} • rustypipe {} • yt-dlp {ytdlp}", env!("CARGO_PKG_VERSION"), rustypipe::VERSION));
                ui.add_space(6.0);
                open_help |= ui::pill(ui, None, tr("Atalhos de teclado (?)"), false).clicked();
            });
            ui.add_space(40.0);
        });
        });
        });
        if let Some(t) = new_tab {
            self.settings_tab = t;
        }
        self.show_shortcuts |= open_help;
        self.acts.extend(acts);
    }

    fn login(&mut self, ui: &mut egui::Ui) {
        self.scan_logins();
        let p = c();
        let browser = crate::login::default_browser();
        let bname = browser.map_or("navegador", crate::login::label);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_max_width(440.0);
                ui.add_space(48.0);
                let (r, _) = ui.allocate_exact_size(vec2(72.0, 72.0), egui::Sense::hover());
                ui::paint_logo(ui.painter(), r);
                ui.add_space(20.0);
                heading(ui, tr("Entre no YtmFast"), 30.0);
                ui.add_space(6.0);
                weak(ui, tr("Use sua conta do Google para ver sua biblioteca, curtidas e playlists, e ouvir na qualidade Premium."));
                ui.add_space(28.0);

                let card = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
                    egui::Frame::NONE.fill(p.bar).corner_radius(12).inner_margin(20).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        add(ui);
                    });
                };

                if let Some((wait_on, _)) = self.login_wait.clone() {
                    // Waiting for the user to finish signing in, in their browser.
                    card(ui, &mut |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(22.0).color(p.accent));
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(tf!("Termine o login no {}", crate::login::label(&wait_on))).font(ui::medium(15.0)).color(p.text));
                                weak(ui, tr("O YtmFast continua sozinho assim que você entrar."));
                            });
                        });
                    });
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui::pill(ui, None, tr("Abrir o login de novo"), false).clicked() {
                            crate::login::open_in(Some(&wait_on), crate::login::SIGN_IN_URL);
                        }
                        if ui::pill(ui, None, tr("Cancelar"), false).clicked() {
                            self.acts.push(Act::LoginCancel);
                        }
                    });
                    // The poll result arrives as a message; keep the spinner turning meanwhile.
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                } else {
                    match &self.login_scan {
                        None => {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(18.0).color(p.text2));
                                weak(ui, tf!("Procurando sua conta no {}…", bname));
                            });
                            ui.add_space(12.0);
                        }
                        Some(found) if !found.is_empty() => {
                            // One click: the session already open in the default browser.
                            let label = crate::login::label(&found[0].0);
                            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), egui::Sense::click());
                            let hov = resp.hovered();
                            ui.painter().rect_filled(rect, 12.0, if hov { p.selected } else { p.bar });
                            let av = egui::Rect::from_center_size(pos2(rect.left() + 40.0, rect.center().y), vec2(44.0, 44.0));
                            ui.painter().circle_filled(av.center(), 22.0, p.accent.gamma_multiply(0.85));
                            ui::paint_icon(ui.painter(), Icon::Person, av.shrink(11.0), Color32::WHITE);
                            let x = av.right() + 16.0;
                            ui.painter().text(pos2(x, rect.center().y - 10.0), egui::Align2::LEFT_CENTER, tf!("Continuar com o {}", label), ui::bold(16.0), p.text);
                            ui.painter().text(pos2(x, rect.center().y + 12.0), egui::Align2::LEFT_CENTER, tr("Sua conta do YouTube já está aberta nele"), ui::regular(13.0), p.text2);
                            ui::paint_icon(ui.painter(), Icon::Forward, egui::Rect::from_center_size(pos2(rect.right() - 28.0, rect.center().y), vec2(20.0, 20.0)), p.text2);
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                self.acts.push(Act::LoginUse(0));
                            }
                            ui.add_space(16.0);
                            weak(ui, tr("ou"));
                            ui.add_space(8.0);
                        }
                        Some(_) => {}
                    }
                    let primary = self.login_scan.as_ref().is_some_and(|f| f.is_empty());
                    if ui::pill(ui, Some(Icon::Person), tr("Entrar com o Google"), primary).clicked() {
                        self.acts.push(Act::LoginGoogle);
                    }
                    ui.add_space(6.0);
                    weak(ui, tf!("Abre o login do Google no {}.", bname));
                }

                ui.add_space(32.0);
                ui.label(RichText::new(tr("Sua senha nunca passa pelo YtmFast. Ficam guardados só os cookies do YouTube, acessíveis apenas pelo seu usuário.")).size(12.0).color(p.text2));
                ui.add_space(20.0);
                egui::CollapsingHeader::new(RichText::new(tr("Opções avançadas")).color(p.text2)).id_salt("login-adv").show(ui, |ui| {
                    weak(ui, tr("Entrar com um arquivo cookies.txt exportado do navegador:"));
                    ui.horizontal(|ui| {
                        egui::Frame::NONE.fill(p.bar).corner_radius(8).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.cookie_path).frame(egui::Frame::NONE).hint_text("/caminho/para/cookies.txt").desired_width(260.0));
                        });
                        if ui::pill(ui, None, tr("Entrar"), false).clicked() {
                            match std::fs::read_to_string(self.cookie_path.trim()) {
                                Err(e) => self.status = tf!("Erro lendo arquivo: {}", e),
                                Ok(txt) => {
                                    self.status = tr("Entrando…").into();
                                    // Only YouTube's cookies are kept, whatever the file holds.
                                    self.login_with(crate::login::filter_youtube(&txt), None, false);
                                }
                            }
                        }
                    });
                });
                ui.add_space(40.0);
            });
        });
    }
}

struct TrackListOpts {
    id: &'static str,
    removable: bool,
    /// Rows can be reordered (owned playlist with row ids, or the queue).
    movable: bool,
    show_album: bool,
    /// Rows are queue entries starting at this queue index.
    queue_offset: Option<usize>,
    /// Clicking plays just that track (plus radio) instead of the whole list.
    solo: bool,
}

/// Winamp 2's built-in presets (from its winamp.q1 library, as shipped with Webamp), converted
/// from its 1..64 scale to dB. Same ten bands as ours. "Plano" and "Vocal" are our own additions.
pub(crate) const EQ_PRESETS: &[(&str, [f32; 10])] = &[
    ("Plano", [0.0; 10]),
    ("Vocal", [-2.0, -1.5, 0.0, 2.5, 4.0, 3.5, 2.0, 0.0, -1.0, -1.5]),
    ("Pop", [-1.5, 2.5, 4.5, 4.5, 3.0, -1.0, -2.0, -2.0, -1.5, -1.5]),
    ("Rock", [4.5, 2.5, -4.0, -5.5, -2.5, 2.5, 5.5, 6.5, 6.5, 6.5]),
    ("Rock suave", [2.5, 2.5, 1.0, -1.0, -3.0, -4.0, -2.5, -1.0, 1.5, 5.5]),
    ("Dance", [6.0, 4.5, 1.0, -0.5, -0.5, -4.5, -5.0, -5.0, -0.5, -0.5]),
    ("Techno", [4.5, 3.5, 0.0, -4.0, -3.5, 0.0, 4.5, 6.0, 6.0, 5.5]),
    ("Clube", [0.0, 0.0, 2.0, 3.5, 3.5, 3.5, 2.0, 0.0, 0.0, 0.0]),
    ("Festa", [4.5, 4.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 4.5, 4.5]),
    ("Reggae", [0.0, 0.0, -1.0, -4.5, 0.0, 4.0, 4.0, 0.0, 0.0, 0.0]),
    ("Ska", [-2.0, -3.5, -3.0, -1.0, 2.5, 3.5, 5.5, 6.0, 6.5, 6.0]),
    ("Clássica", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -5.0, -5.0, -5.0, -6.5]),
    ("Ao vivo", [-3.5, 0.0, 2.5, 3.0, 3.5, 3.5, 2.5, 1.5, 1.5, 1.0]),
    ("Grande salão", [6.0, 6.0, 3.5, 3.5, 0.0, -3.5, -3.5, -3.5, 0.0, 0.0]),
    ("Suave", [2.5, 1.0, -1.0, -2.0, -1.0, 2.5, 5.0, 6.0, 6.5, 7.5]),
    ("Fones e notebook", [2.5, 6.5, 3.0, -2.5, -2.0, 1.0, 2.5, 6.0, 7.5, 9.0]),
    ("Graves intensos", [6.0, 6.0, 6.0, 3.5, 1.0, -3.0, -6.0, -7.0, -7.5, -7.5]),
    ("Agudos intensos", [-6.5, -6.5, -6.5, -3.0, 1.5, 6.5, 9.5, 9.5, 9.5, 10.5]),
    ("Graves e agudos", [4.5, 3.5, 0.0, -5.0, -3.5, 1.0, 5.0, 6.5, 7.5, 7.5]),
];

const SETTINGS_TABS: [(Icon, &str); 10] = [
    (Icon::Play, "Reprodução"),
    (Icon::Wave, "Equalizador"),
    (Icon::Gear, "Aparência"),
    (Icon::Mini, "Janela"),
    (Icon::Heart, "Integrações"),
    (Icon::Mini, "Winamp"),
    (Icon::Person, "Conta"),
    (Icon::Radio, "Rede"),
    (Icon::Queue, "Armazenamento"),
    (Icon::Lyrics, "Sobre"),
];

/// Preamp + 10 bands as drag columns (-12..12 dB), with the response curve drawn through them.
fn eq_editor(ui: &mut egui::Ui, preamp: &mut f32, bands: &mut [f32; 10]) {
    let p = c();
    let h = 170.0;
    let w = ui.available_width();
    let (area, _) = ui.allocate_exact_size(vec2(w, h + 44.0), egui::Sense::hover());
    let painter = ui.painter();
    let top = area.top() + 18.0;
    let y_of = |db: f32| top + (12.0 - db) / 24.0 * h;
    // 0 dB guide and the ±12 marks.
    for (db, label) in [(12.0, "+12"), (0.0, "0"), (-12.0, "-12")] {
        let y = y_of(db);
        painter.line_segment([pos2(area.left() + 30.0, y), pos2(area.right(), y)], egui::Stroke::new(1.0, p.line));
        painter.text(pos2(area.left(), y), egui::Align2::LEFT_CENTER, label, ui::regular(10.0), p.text2);
    }
    let left = area.left() + 44.0;
    let col = (area.right() - left) / 11.0;
    let mut knobs = vec![];
    let names = [tr("Pré"), "60", "170", "310", "600", "1k", "3k", "6k", "12k", "14k", "16k"];
    for i in 0..11 {
        let v: &mut f32 = if i == 0 { &mut *preamp } else { &mut bands[i - 1] };
        let x = left + col * (i as f32 + 0.5);
        let hit = Rect::from_center_size(pos2(x, top + h / 2.0), vec2(col.min(44.0), h + 12.0));
        let resp = ui.interact(hit, ui.id().with(("eq", i)), egui::Sense::click_and_drag());
        if let Some(pp) = resp.interact_pointer_pos() {
            // Half-dB steps; double-click resets to flat.
            *v = ((12.0 - (pp.y - top) / h * 24.0).clamp(-12.0, 12.0) * 2.0).round() / 2.0;
        }
        if resp.double_clicked() {
            *v = 0.0;
        }
        let painter = ui.painter();
        let y = y_of(*v);
        painter.line_segment([pos2(x, top), pos2(x, top + h)], egui::Stroke::new(4.0, p.selected));
        painter.line_segment([pos2(x, y_of(0.0)), pos2(x, y)], egui::Stroke::new(4.0, p.accent));
        let hot = resp.hovered() || resp.dragged();
        painter.circle_filled(pos2(x, y), if hot { 8.0 } else { 6.5 }, p.text);
        let val = if *v == 0.0 { "0".to_string() } else { format!("{:+.1}", v) };
        painter.text(pos2(x, area.top()), egui::Align2::CENTER_TOP, val, ui::medium(11.0), if hot { p.text } else { p.text2 });
        painter.text(pos2(x, top + h + 14.0), egui::Align2::CENTER_TOP, names[i], ui::regular(11.0), p.text2);
        if i == 0 {
            // The preamp sits apart from the frequency curve.
            painter.line_segment([pos2(x + col / 2.0, top), pos2(x + col / 2.0, top + h)], egui::Stroke::new(1.0, p.line));
        } else {
            knobs.push(pos2(x, y));
        }
    }
    ui.painter().add(egui::epaint::PathShape::line(knobs, egui::Stroke::new(2.0, p.accent.gamma_multiply(0.6))));
}

/// Label on the left, control on the right.
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        ui.label(RichText::new(label).font(ui::medium(14.0)).color(c().text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

fn choice<T: PartialEq + Copy>(ui: &mut egui::Ui, label: &str, value: &mut T, options: &[(T, &str)]) {
    let current = options.iter().find(|(v, _)| v == value).map_or("—", |(_, n)| n);
    row(ui, label, |ui| {
        egui::ComboBox::from_id_salt(label).selected_text(current).width(200.0).show_ui(ui, |ui| {
            for (v, name) in options {
                ui.selectable_value(value, *v, *name);
            }
        });
    });
}

/// "12,3 mil" / "2,5 mi" in Portuguese, "12.3K" / "2.5M" in English.
fn compact_num(n: u64) -> String {
    compact_num_in(n, crate::i18n::english())
}

fn compact_num_in(n: u64, english: bool) -> String {
    let (v, unit) = match n {
        0..1_000 => return n.to_string(),
        1_000..1_000_000 => (n as f64 / 1e3, if english { "K" } else { " mil" }),
        _ => (n as f64 / 1e6, if english { "M" } else { " mi" }),
    };
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").unwrap_or(&s);
    format!("{}{unit}", if english { s.to_string() } else { s.replace('.', ",") })
}

#[cfg(test)]
mod tests {
    use super::compact_num_in;

    #[test]
    fn numbers() {
        assert_eq!(compact_num_in(950, false), "950");
        assert_eq!(compact_num_in(1_000, false), "1 mil");
        assert_eq!(compact_num_in(12_345, false), "12,3 mil");
        assert_eq!(compact_num_in(2_500_000, false), "2,5 mi");
        assert_eq!(compact_num_in(12_345, true), "12.3K");
        assert_eq!(compact_num_in(2_000_000, true), "2M");
    }
}

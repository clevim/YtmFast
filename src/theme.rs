//! Colour palettes and the global look (palette + effect flags) the widgets read every frame.

use std::sync::RwLock;

use eframe::egui::{self, Color32, CornerRadius, Stroke};
use serde::Deserialize;

use crate::settings::config_dir;

#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub bar: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub line: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub accent: Color32,
}

impl Palette {
    /// Translucent wash for hover states that works on light and dark backgrounds.
    pub fn wash(&self) -> Color32 {
        self.text.gamma_multiply(0.09)
    }
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: rgb(0x030303),
    bar: rgb(0x212121),
    hover: rgb(0x1c1c1c),
    selected: rgb(0x282828),
    line: rgb(0x262626),
    text: rgb(0xf1f1f1),
    text2: rgb(0xaaaaaa),
    accent: rgb(0xff0033),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: rgb(0xffffff),
    bar: rgb(0xf2f2f2),
    hover: rgb(0xf0f0f0),
    selected: rgb(0xe5e5e5),
    line: rgb(0xe0e0e0),
    text: rgb(0x0f0f0f),
    text2: rgb(0x606060),
    accent: rgb(0xe6002e),
};

/// Effect knobs from settings, read by widgets without threading settings everywhere.
#[derive(Clone, Copy)]
pub struct Look {
    pub pal: Palette,
    /// 0 disables the 3D tilt entirely.
    pub tilt: f32,
    pub motion: bool,
}

static LOOK: RwLock<Look> = RwLock::new(Look { pal: DARK, tilt: 1.0, motion: true });

pub fn c() -> Palette {
    LOOK.read().unwrap().pal
}
/// Runs `f` with `pal` as the palette (the compact player's LCD style), then puts the theme's back.
pub fn with_palette<R>(pal: Palette, f: impl FnOnce() -> R) -> R {
    let old = std::mem::replace(&mut LOOK.write().unwrap().pal, pal);
    let r = f();
    LOOK.write().unwrap().pal = old;
    r
}

pub fn look() -> Look {
    *LOOK.read().unwrap()
}

/// Spotifast-compatible palette file: `{"base": "dark", "colors": {"window": "#RRGGBB", ...}}`.
#[derive(Deserialize)]
struct PaletteFile {
    #[serde(default)]
    base: String,
    #[serde(default)]
    colors: std::collections::HashMap<String, String>,
}

fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        6 => Some(rgb(v)),
        8 => Some(Color32::from_rgba_unmultiplied((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)),
        _ => None,
    }
}

pub fn parse_palette(json: &str) -> Option<Palette> {
    let f: PaletteFile = serde_json::from_str(json).ok()?;
    let mut p = if f.base == "light" { LIGHT } else { DARK };
    for (k, v) in &f.colors {
        let Some(col) = parse_hex(v) else { continue };
        match k.as_str() {
            "window" => p.bg = col,
            "panel" => p.bar = col,
            "surface_hover" => p.hover = col,
            "surface_active" => p.selected = col,
            "outline" => p.line = col,
            "text" => p.text = col,
            "secondary" => p.text2 = col,
            "accent" => p.accent = col,
            _ => {}
        }
    }
    Some(p)
}

pub fn themes_dir() -> std::path::PathBuf {
    config_dir().join("themes")
}

const BUNDLED: [(&str, &str); 15] = [
    ("catppuccin-mocha.json", r##"{"base":"dark","colors":{"window":"#1e1e2e","panel":"#181825","surface_hover":"#313244","surface_active":"#45475a","outline":"#313244","text":"#cdd6f4","secondary":"#a6adc8","accent":"#cba6f7"}}"##),
    ("catppuccin-latte.json", r##"{"base":"light","colors":{"window":"#eff1f5","panel":"#e6e9ef","surface_hover":"#dce0e8","surface_active":"#ccd0da","outline":"#ccd0da","text":"#4c4f69","secondary":"#6c6f85","accent":"#8839ef"}}"##),
    ("nord.json", r##"{"base":"dark","colors":{"window":"#2e3440","panel":"#3b4252","surface_hover":"#434c5e","surface_active":"#4c566a","outline":"#434c5e","text":"#eceff4","secondary":"#d8dee9","accent":"#88c0d0"}}"##),
    ("tokyo-night.json", r##"{"base":"dark","colors":{"window":"#1a1b26","panel":"#16161e","surface_hover":"#24283b","surface_active":"#2f3549","outline":"#24283b","text":"#c0caf5","secondary":"#9aa5ce","accent":"#7aa2f7"}}"##),
    ("rose-pine.json", r##"{"base":"dark","colors":{"window":"#191724","panel":"#1f1d2e","surface_hover":"#26233a","surface_active":"#403d52","outline":"#26233a","text":"#e0def4","secondary":"#908caa","accent":"#ebbcba"}}"##),
    ("rose-pine-moon.json", r##"{"base":"dark","colors":{"window":"#232136","panel":"#2a273f","surface_hover":"#393552","surface_active":"#44415a","outline":"#393552","text":"#e0def4","secondary":"#908caa","accent":"#ea9a97"}}"##),
    ("rose-pine-dawn.json", r##"{"base":"light","colors":{"window":"#faf4ed","panel":"#fffaf3","surface_hover":"#f2e9e1","surface_active":"#dfdad9","outline":"#f2e9e1","text":"#575279","secondary":"#797593","accent":"#d7827e"}}"##),
    ("gruvbox-dark.json", r##"{"base":"dark","colors":{"window":"#282828","panel":"#1d2021","surface_hover":"#3c3836","surface_active":"#504945","outline":"#3c3836","text":"#ebdbb2","secondary":"#a89984","accent":"#fe8019"}}"##),
    ("gruvbox-light.json", r##"{"base":"light","colors":{"window":"#fbf1c7","panel":"#f2e5bc","surface_hover":"#ebdbb2","surface_active":"#d5c4a1","outline":"#ebdbb2","text":"#3c3836","secondary":"#7c6f64","accent":"#af3a03"}}"##),
    ("dracula.json", r##"{"base":"dark","colors":{"window":"#282a36","panel":"#21222c","surface_hover":"#343746","surface_active":"#44475a","outline":"#343746","text":"#f8f8f2","secondary":"#b6b8cc","accent":"#bd93f9"}}"##),
    ("everforest.json", r##"{"base":"dark","colors":{"window":"#2d353b","panel":"#272e33","surface_hover":"#343f44","surface_active":"#3d484d","outline":"#343f44","text":"#d3c6aa","secondary":"#9da9a0","accent":"#a7c080"}}"##),
    ("kanagawa.json", r##"{"base":"dark","colors":{"window":"#1f1f28","panel":"#16161d","surface_hover":"#2a2a37","surface_active":"#363646","outline":"#2a2a37","text":"#dcd7ba","secondary":"#c8c093","accent":"#7e9cd8"}}"##),
    ("one-dark.json", r##"{"base":"dark","colors":{"window":"#282c34","panel":"#21252b","surface_hover":"#2c313a","surface_active":"#3e4451","outline":"#2c313a","text":"#abb2bf","secondary":"#7f848e","accent":"#61afef"}}"##),
    ("solarized-dark.json", r##"{"base":"dark","colors":{"window":"#002b36","panel":"#00212b","surface_hover":"#073642","surface_active":"#0b4250","outline":"#073642","text":"#93a1a1","secondary":"#839496","accent":"#268bd2"}}"##),
    ("solarized-light.json", r##"{"base":"light","colors":{"window":"#fdf6e3","panel":"#eee8d5","surface_hover":"#e9e2cc","surface_active":"#ddd6c1","outline":"#e4ddc8","text":"#586e75","secondary":"#839496","accent":"#268bd2"}}"##),
];

/// Writes each bundled palette once; ones the user deletes are not recreated. The marker lists the
/// names already handed out, so palettes added in later versions still arrive.
pub fn install_bundled() {
    let dir = themes_dir();
    let marker = dir.join(".installed-palettes");
    let listed = std::fs::read_to_string(&marker).ok();
    let done = installed_names(listed.as_deref());
    if BUNDLED.iter().all(|(n, _)| done.contains(n)) {
        return;
    }
    std::fs::create_dir_all(&dir).ok();
    for (name, json) in BUNDLED {
        if !done.contains(&name) && !dir.join(name).exists() {
            std::fs::write(dir.join(name), json).ok();
        }
    }
    let names: Vec<&str> = BUNDLED.iter().map(|(n, _)| *n).collect();
    std::fs::write(marker, names.join("\n") + "\n").ok();
}

/// Names the marker says were installed. An empty marker is from before it listed them: the first
/// five palettes.
fn installed_names(marker: Option<&str>) -> Vec<&str> {
    match marker {
        None => vec![],
        Some(m) if m.trim().is_empty() => BUNDLED[..5].iter().map(|(n, _)| *n).collect(),
        Some(m) => m.lines().map(str::trim).filter(|l| !l.is_empty()).collect(),
    }
}

/// Palette files the user can pick, sorted by name.
pub fn custom_themes() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(themes_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".json"))
        .collect();
    v.sort();
    v
}

pub fn resolve(theme: &str, system_dark: bool) -> Palette {
    match theme {
        "dark" => DARK,
        "light" => LIGHT,
        "system" => if system_dark { DARK } else { LIGHT },
        file => Some(themes_dir().join(file))
            // Same limit as Spotifast: palette files are tiny.
            .filter(|p| crate::settings::safe_name(file) && std::fs::metadata(p).is_ok_and(|m| m.len() <= 64 << 10))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|j| parse_palette(&j))
            .unwrap_or(if system_dark { DARK } else { LIGHT }),
    }
}

pub fn apply(ctx: &egui::Context, look: Look) {
    *LOOK.write().unwrap() = look;
    let p = look.pal;
    let theme = if p.dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(theme);
    ctx.style_mut_of(theme, |s| {
        s.animation_time = if look.motion { 1.0 / 12.0 } else { 0.0 };
        let v = &mut s.visuals;
        v.dark_mode = p.dark;
        v.panel_fill = p.bg;
        v.window_fill = p.bar;
        v.extreme_bg_color = p.bar;
        v.faint_bg_color = p.hover;
        v.window_stroke = Stroke::new(1.0, p.line);
        v.window_corner_radius = CornerRadius::same(8);
        v.menu_corner_radius = CornerRadius::same(8);
        v.popup_shadow = egui::Shadow::NONE;
        v.widgets.noninteractive.fg_stroke.color = p.text;
        v.widgets.noninteractive.bg_stroke.color = p.line;
        v.widgets.noninteractive.bg_fill = p.bar;
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(6);
            w.bg_stroke = Stroke::NONE;
            w.expansion = 0.0;
            w.fg_stroke.color = p.text;
        }
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_fill = p.selected;
        v.widgets.hovered.weak_bg_fill = p.selected;
        v.widgets.hovered.bg_fill = p.line;
        v.widgets.active.weak_bg_fill = p.line;
        v.widgets.active.bg_fill = p.text2;
        v.widgets.open.weak_bg_fill = p.selected;
        v.selection.bg_fill = p.accent.gamma_multiply(0.45);
        v.selection.stroke = Stroke::new(1.0, p.text);
        v.text_cursor.stroke = Stroke::new(2.0, p.text);
        v.slider_trailing_fill = true;
    });
}

/// Theme change: a screenshot of the old colours is laid over the new ones and cleared away. The
/// capture-then-reveal mechanism is adapted from fastframe-theme's `Transition` (MIT, Carmine
/// Paolino, as in Spotifast); the five reveals are our own and take turns, one per change.
/// `begin` + `holding` before applying the new palette, `paint` after the frame is drawn.
#[derive(Default)]
pub enum Transition {
    #[default]
    Idle,
    Capturing { since: f64 },
    Revealing { picture: egui::TextureHandle, start: f64, reveal: Reveal },
}

/// Marks our screenshots, so other screenshot users aren't confused.
struct TransitionShot;

/// How the old colours clear away. They take turns, one per theme change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reveal {
    /// A soft diagonal wipe from the top-left corner.
    Sweep,
    /// The old picture rises like a curtain with a wavy hem.
    Curtain,
    /// A honeycomb: hexagons shrink away from the middle outwards.
    Hexagons,
    /// Horizontal slats closing one after another.
    Blinds,
    /// Tiles fading out in a scattered order.
    Mosaic,
}

const REVEALS: [Reveal; 5] = [Reveal::Sweep, Reveal::Curtain, Reveal::Hexagons, Reveal::Blinds, Reveal::Mosaic];
static NEXT_REVEAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Reveal {
    /// Grid the screen is cut into: fine enough for smooth edges, cells for the tiled ones.
    fn grid(self) -> (usize, usize) {
        match self {
            Self::Blinds => (1, 12 * 8),
            Self::Mosaic => (16, 10),
            _ => (48, 30),
        }
    }

    /// How much of the old picture still shows (0..1) at (u, v) on screen (0..1 each), in cell
    /// (col, row), at `p` (0 = all old, 1 = all new).
    fn cover(self, u: f32, v: f32, (col, row): (usize, usize), p: f32) -> f32 {
        let tau = std::f32::consts::TAU;
        match self {
            Self::Sweep => {
                let d = (u + v) / 2.0;
                let edge = p * 1.3 - 0.15;
                smooth(edge, edge + 0.15, d)
            }
            Self::Curtain => {
                let hem = 1.07 - p * 1.32 + 0.04 * (u * tau * 2.0 + p * 7.0).sin();
                1.0 - smooth(hem - 0.03, hem + 0.03, v)
            }
            Self::Hexagons => {
                // Per hexagon (its centre at u, v): the middle goes first, with a little scatter.
                let (dx, dy) = (u - 0.5, v - 0.5);
                let h = ((col * 73 + row * 151) % 97) as f32 / 97.0;
                let start = ((dx * dx + dy * dy).sqrt() / 0.71 * 0.5 + h * 0.1).min(0.6);
                1.0 - smooth(start, start + 0.4, p)
            }
            Self::Blinds => {
                // Twelve slats, each cleared top-down; the lower ones start a little later.
                let slat = row / 8;
                let local = v * 12.0 - slat as f32;
                let start = slat as f32 / 12.0 * 0.4;
                let edge = ((p - start) / 0.6).clamp(0.0, 1.0) * 1.06;
                smooth(edge - 0.05, edge, local)
            }
            Self::Mosaic => {
                // A fixed scatter per tile (cheap hash), each fading over a third of the time.
                let h = ((col * 73 + row * 151) % 97) as f32 / 97.0;
                let start = h * 0.65;
                1.0 - smooth(start, start + 0.35, p)
            }
        }
    }

    /// The old picture as a mesh of cells, each corner as visible as `cover` says.
    fn mesh(self, screen: egui::Rect, texture: egui::TextureId, p: f32) -> egui::Mesh {
        if self == Self::Hexagons {
            return hexagons(screen, texture, p);
        }
        let (cols, rows) = self.grid();
        let mut mesh = egui::Mesh::with_texture(texture);
        for row in 0..rows {
            for col in 0..cols {
                let base = mesh.vertices.len() as u32;
                for (dc, dr) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                    let (u, v) = ((col + dc) as f32 / cols as f32, (row + dr) as f32 / rows as f32);
                    let a = self.cover(u, v, (col, row), p);
                    let pos = egui::pos2(screen.min.x + u * screen.width(), screen.min.y + v * screen.height());
                    mesh.vertices.push(egui::epaint::Vertex { pos, uv: egui::pos2(u, v), color: Color32::WHITE.gamma_multiply(a) });
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
        }
        mesh
    }
}

/// Pointy-top hexagons tiling the screen, each shrinking to its centre and fading as `cover` says.
fn hexagons(screen: egui::Rect, texture: egui::TextureId, p: f32) -> egui::Mesh {
    const R: f32 = 46.0;
    let (w, step) = (3f32.sqrt() * R, 1.5 * R);
    let (cols, rows) = ((screen.width() / w).ceil() as usize + 2, (screen.height() / step).ceil() as usize + 2);
    let mut mesh = egui::Mesh::with_texture(texture);
    let uv = |q: egui::Pos2| egui::pos2((q.x - screen.min.x) / screen.width(), (q.y - screen.min.y) / screen.height());
    for row in 0..rows {
        for col in 0..cols {
            let x = screen.min.x + col as f32 * w - if row % 2 == 1 { w / 2.0 } else { 0.0 };
            let centre = egui::pos2(x, screen.min.y + row as f32 * step);
            let c = uv(centre);
            let a = Reveal::Hexagons.cover(c.x, c.y, (col, row), p);
            if a <= 0.0 {
                continue;
            }
            // Slightly oversized at rest so neighbours overlap instead of leaving hairline seams.
            let r = R * (0.15 + 0.85 * a) + 0.5;
            let color = Color32::WHITE.gamma_multiply(a);
            let base = mesh.vertices.len() as u32;
            mesh.vertices.push(egui::epaint::Vertex { pos: centre, uv: c, color });
            for k in 0..6 {
                let ang = std::f32::consts::FRAC_PI_3 * k as f32 + std::f32::consts::FRAC_PI_6;
                let q = centre + r * egui::vec2(ang.cos(), ang.sin());
                mesh.vertices.push(egui::epaint::Vertex { pos: q, uv: uv(q), color });
            }
            for k in 0..6 {
                mesh.add_triangle(base, base + 1 + k, base + 1 + (k + 1) % 6);
            }
        }
    }
    mesh
}

impl Transition {
    const WAIT: f64 = 0.25;
    const DURATION: f64 = 0.55;

    pub fn begin(&mut self, ctx: &egui::Context) {
        if matches!(self, Self::Capturing { .. }) {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(TransitionShot)));
        *self = Self::Capturing { since: ctx.input(|i| i.time) };
        ctx.request_repaint();
    }

    /// True while the old colours must stay up (screenshot on its way, at most WAIT seconds).
    pub fn holding(&mut self, ctx: &egui::Context) -> bool {
        let Self::Capturing { since } = *self else { return false };
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { user_data, image, .. } if user_data.data.as_ref().is_some_and(|d| d.downcast_ref::<TransitionShot>().is_some()) => Some(image.clone()),
                _ => None,
            })
        });
        let now = ctx.input(|i| i.time);
        if let Some(image) = shot {
            let picture = ctx.load_texture("theme-transition", std::sync::Arc::unwrap_or_clone(image), egui::TextureOptions::LINEAR);
            let reveal = REVEALS[NEXT_REVEAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % REVEALS.len()];
            *self = Self::Revealing { picture, start: now, reveal };
            ctx.request_repaint();
            return false;
        }
        if now - since > Self::WAIT {
            *self = Self::Idle;
            return false;
        }
        ctx.request_repaint();
        true
    }

    pub fn paint(&mut self, ctx: &egui::Context) {
        let Self::Revealing { picture, start, reveal } = self else { return };
        let t = ((ctx.input(|i| i.time) - *start) / Self::DURATION) as f32;
        if t >= 1.0 {
            *self = Self::Idle;
            return;
        }
        let progress = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("theme-transition")));
        painter.add(reveal.mesh(ctx.viewport_rect(), picture.id(), progress));
        ctx.request_repaint();
    }
}

/// "catppuccin-mocha.json" -> "Catppuccin Mocha".
pub fn display_name(file: &str) -> String {
    file.trim_end_matches(".json")
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_names() {
        assert_eq!(display_name("catppuccin-mocha.json"), "Catppuccin Mocha");
        assert_eq!(display_name("Rose Pine Dawn.json"), "Rose Pine Dawn");
        // Every reveal starts fully old and ends fully new.
        let r = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        for rv in super::REVEALS {
            let start = rv.mesh(r, egui::TextureId::default(), 0.0);
            let end = rv.mesh(r, egui::TextureId::default(), 1.0);
            assert!(start.vertices.iter().all(|v| v.color.a() > 250), "{rv:?} at 0");
            assert!(end.vertices.iter().all(|v| v.color.a() < 5), "{rv:?} at 1");
        }
    }

    #[test]
    fn palette_files() {
        let p = parse_palette(r##"{"base":"light","colors":{"accent":"#112233","text":"#00000080","nope":"x"}}"##).unwrap();
        assert!(!p.dark);
        assert_eq!(p.accent, rgb(0x112233));
        assert_eq!(p.text, Color32::from_rgba_unmultiplied(0, 0, 0, 0x80));
        assert_eq!(p.bg, LIGHT.bg);
        for (_, json) in BUNDLED {
            assert!(parse_palette(json).is_some());
        }
        // Old empty marker: the first five were handed out, the newer ones still arrive.
        assert_eq!(installed_names(Some("")).len(), 5);
        assert!(!installed_names(Some("")).contains(&"dracula.json"));
        assert_eq!(installed_names(Some("nord.json\ndracula.json\n")), ["nord.json", "dracula.json"]);
        assert!(installed_names(None).is_empty());
        assert!(parse_palette("not json").is_none());
    }
}

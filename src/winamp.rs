//! Classic Winamp 2 main window drawn from a `.wsz` skin (sprite layout as documented by Webamp).
//! Skins are read in memory only (never extracted), with size limits.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};

use crate::settings::config_dir;

pub const W: f32 = 275.0;
pub const H: f32 = 116.0;
const MAX_SKIN: u64 = 20 << 20;
const MAX_ENTRY: u64 = 4 << 20;
const SHEETS: [&str; 11] = ["main", "titlebar", "cbuttons", "posbar", "volume", "balance", "shufrep", "playpaus", "monoster", "numbers", "text"];

pub const CLASSIC_NAME: &str = "Winamp clássico.wsz";
/// Not a file: the classic skin recoloured with the current theme.
pub const THEMED: &str = "*theme";

const SOURCE: &str = "https://raw.githubusercontent.com/captbaritone/webamp/master/packages/";
/// Skins offered for download (never bundled): (file name in the skins folder, path in Webamp's
/// repository). First Winamp 2.91's base skin, then the ones Webamp's own demo offers.
pub const GALLERY: [(&str, &str); 8] = [
    (CLASSIC_NAME, "webamp/assets/skins/base-2.91.wsz"),
    ("Green Dimension V2.wsz", "webamp-demo/skins/Green-Dimension-V2.wsz"),
    ("Internet Archive.wsz", "webamp-demo/skins/Internet-Archive.wsz"),
    ("Mac OS X Aqua.wsz", "webamp-demo/skins/MacOSXAqua1-5.wsz"),
    ("TopazAmp.wsz", "webamp-demo/skins/TopazAmp1-2.wsz"),
    ("Vizor.wsz", "webamp-demo/skins/Vizor1-01.wsz"),
    ("XMMS Turquoise.wsz", "webamp-demo/skins/XMMS-Turquoise.wsz"),
    ("Zaxon Remake.wsz", "webamp-demo/skins/ZaxonRemake1-0.wsz"),
];

/// Downloads `GALLERY[i]` into the skins folder. Blocking; run off the UI thread.
pub fn download(i: usize) -> Result<String, String> {
    let (name, path) = GALLERY.get(i).ok_or("?")?;
    let r = ehttp::fetch_blocking(&ehttp::Request::get(format!("{SOURCE}{path}")))?;
    if !r.ok {
        return Err(format!("HTTP {}", r.status));
    }
    // A .wsz is a zip: check the magic and size before keeping it.
    if r.bytes.len() as u64 > MAX_SKIN || !r.bytes.starts_with(b"PK") {
        return Err(crate::i18n::tr("arquivo inválido").into());
    }
    std::fs::create_dir_all(skins_dir()).map_err(|e| e.to_string())?;
    std::fs::write(skins_dir().join(name), &r.bytes).map_err(|e| e.to_string())?;
    Ok(name.to_string())
}

pub fn skins_dir() -> PathBuf {
    config_dir().join("skins")
}

/// `.wsz` files (or unzipped skin folders) in the skins folder.
pub fn list_skins() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(skins_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let is_wsz = name.to_ascii_lowercase().ends_with(".wsz") || name.to_ascii_lowercase().ends_with(".zip");
            (is_wsz || e.file_type().ok()?.is_dir()).then_some(name)
        })
        .collect();
    v.sort_by_key(|s| s.to_ascii_lowercase());
    v
}

pub struct Skin {
    sheets: HashMap<&'static str, (TextureHandle, egui::Vec2)>,
    vis: [Color32; 24],
}

const DEFAULT_VIS: [(u8, u8, u8); 24] = [
    (0, 0, 0), (24, 33, 41), (239, 49, 16), (206, 41, 16), (214, 90, 0), (214, 102, 0), (214, 115, 0), (198, 123, 8),
    (222, 165, 24), (214, 181, 33), (189, 222, 41), (148, 222, 33), (41, 206, 16), (50, 190, 16), (57, 181, 16), (49, 156, 8),
    (41, 148, 0), (24, 132, 8), (255, 255, 255), (214, 214, 222), (181, 189, 189), (160, 170, 175), (148, 156, 165), (150, 150, 150),
];

fn parse_viscolor(txt: &str) -> [Color32; 24] {
    let mut out = DEFAULT_VIS.map(|(r, g, b)| Color32::from_rgb(r, g, b));
    for (slot, line) in out.iter_mut().zip(txt.lines().filter(|l| l.trim_start().chars().next().is_some_and(|c| c.is_ascii_digit()))) {
        let n: Vec<u8> = line.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).take(3).filter_map(|s| s.parse().ok()).collect();
        if let [r, g, b] = n[..] {
            *slot = Color32::from_rgb(r, g, b);
        }
    }
    out
}

/// Reads every file of a skin (zip or folder) whose lowercase stem we care about.
fn read_files(path: &Path) -> Result<HashMap<String, Vec<u8>>, String> {
    let wanted = |name: &str| {
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
        let ok = file == "viscolor.txt" || SHEETS.iter().any(|s| file == format!("{s}.bmp")) || file == "nums_ex.bmp";
        ok.then_some(file)
    };
    let mut out = HashMap::new();
    if path.is_dir() {
        for e in std::fs::read_dir(path).map_err(|e| e.to_string())?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(key) = wanted(&name)
                && e.metadata().is_ok_and(|m| m.len() <= MAX_ENTRY)
            {
                out.insert(key, std::fs::read(e.path()).map_err(|e| e.to_string())?);
            }
        }
        return Ok(out);
    }
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_SKIN {
        return Err(crate::i18n::tr("skin grande demais").into());
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| e.to_string())?;
        let Ok(name) = f.name().map(|n| n.into_owned()) else { continue };
        if let Some(key) = wanted(&name)
            && !out.contains_key(&key)
        {
            let mut buf = vec![];
            // take() caps what a lying header could make us inflate.
            f.take(MAX_ENTRY).read_to_end(&mut buf).map_err(|e| e.to_string())?;
            out.insert(key, buf);
        }
    }
    Ok(out)
}

impl Skin {
    pub fn load(ctx: &egui::Context, file: &str) -> Result<Self, String> {
        let themed = (file == THEMED).then(crate::theme::c);
        let file = if themed.is_some() { CLASSIC_NAME } else { file };
        if !crate::settings::safe_name(file) {
            return Err(crate::i18n::tr("nome de skin inválido").into());
        }
        let path = skins_dir().join(file);
        let mut files = read_files(&path)?;
        if !files.contains_key("numbers.bmp")
            && let Some(ex) = files.remove("nums_ex.bmp") {
                files.insert("numbers.bmp".into(), ex);
            }
        let mut sheets = HashMap::new();
        for name in SHEETS {
            let Some(bytes) = files.get(&format!("{name}.bmp")) else { continue };
            let Ok(img) = image::load_from_memory(bytes) else { continue };
            if img.width() > 2048 || img.height() > 2048 {
                continue;
            }
            let mut rgba = img.to_rgba8();
            if let Some(p) = &themed {
                let ramp = Ramp::new(p);
                for px in rgba.pixels_mut() {
                    let [r, g, b, _] = px.0;
                    let c = ramp.map(Color32::from_rgb(r, g, b));
                    px.0[..3].copy_from_slice(&[c.r(), c.g(), c.b()]);
                }
            }
            let size = [rgba.width() as usize, rgba.height() as usize];
            let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            let tex = ctx.load_texture(format!("winamp-{name}"), ci, TextureOptions::NEAREST);
            sheets.insert(name, (tex, vec2(size[0] as f32, size[1] as f32)));
        }
        if !sheets.contains_key("main") {
            return Err(crate::i18n::tr("skin sem main.bmp").into());
        }
        let mut vis = files.get("viscolor.txt").map(|b| parse_viscolor(&String::from_utf8_lossy(b))).unwrap_or(DEFAULT_VIS.map(|(r, g, b)| Color32::from_rgb(r, g, b)));
        if let Some(p) = &themed {
            let ramp = Ramp::new(p);
            vis = vis.map(|c| ramp.map(c));
        }
        Ok(Self { sheets, vis })
    }
}

/// Recolours a skin with a theme: greys follow the palette from its darkest colour to its lightest
/// (so bevels keep their light and shadow, in light themes too), and coloured pixels (the green
/// display text, the analyser) take the accent at their brightness.
struct Ramp {
    stops: Vec<Color32>,
    accent: Color32,
}

impl Ramp {
    fn new(p: &crate::theme::Palette) -> Self {
        let lum = |c: &Color32| 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
        let mut stops = vec![p.bg, p.bar, p.line, p.text2, p.text];
        stops.sort_by(|a, b| lum(a).total_cmp(&lum(b)));
        // A near-black below the palette: deep shadows, and a dark display the accent stands out on
        // (a light theme's darkest colour is only its text).
        stops.insert(0, stops[0].lerp_to_gamma(Color32::BLACK, 0.7));
        Self { stops, accent: p.accent }
    }

    fn map(&self, c: Color32) -> Color32 {
        let (r, g, b) = (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let darkest = self.stops[0];
        if max > 0.12 && (max - min) / max > 0.35 {
            // Coloured: the accent, dimmed as the original was.
            return darkest.lerp_to_gamma(self.accent, (max / 0.85).min(1.0));
        }
        let l = (0.2126 * r + 0.7152 * g + 0.0722 * b) * (self.stops.len() - 1) as f32;
        let i = (l.floor() as usize).min(self.stops.len() - 2);
        self.stops[i].lerp_to_gamma(self.stops[i + 1], l - i as f32)
    }
}

/// What the window shows; filled by the app each frame.
pub struct State<'a> {
    pub title: &'a str,
    pub time: f64,
    pub dur: f64,
    pub playing: bool,
    pub paused: bool,
    pub volume: f32,
    pub balance: f32,
    pub shuffle: bool,
    pub repeat: bool,
    pub kbps: u32,
    pub khz: u32,
    pub stereo: bool,
    pub bars: &'a [f32; 19],
}

pub enum Event {
    Prev,
    Play,
    Pause,
    Stop,
    Next,
    /// Eject / close: back to the full app.
    Exit,
    Minimize,
    Seek(f32),
    Volume(f32),
    Balance(f32),
    Shuffle,
    Repeat,
}

struct Painter<'a> {
    ui: &'a Ui,
    skin: &'a Skin,
    origin: Pos2,
    s: f32,
}

impl Painter<'_> {
    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.origin + vec2(x, y) * self.s, vec2(w, h) * self.s)
    }
    /// Draws `src` (skin pixels) of `sheet` at skin position (x, y).
    fn sprite(&self, sheet: &str, src: [f32; 4], x: f32, y: f32) {
        let Some((tex, size)) = self.skin.sheets.get(sheet) else { return };
        let uv = Rect::from_min_size(pos2(src[0] / size.x, src[1] / size.y), vec2(src[2] / size.x, src[3] / size.y));
        self.ui.painter().image(tex.id(), self.rect(x, y, src[2], src[3]), uv, Color32::WHITE);
    }
    fn fill(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.ui.painter().rect_filled(self.rect(x, y, w, h), 0.0, c);
    }
}

/// Position of a character in text.bmp (row, column), Webamp's FONT_LOOKUP.
fn glyph(c: char) -> (u32, u32) {
    let c = c.to_ascii_lowercase();
    match c {
        'a'..='z' => (0, c as u32 - 'a' as u32),
        '0'..='9' => (1, c as u32 - '0' as u32),
        '"' => (0, 26),
        '@' => (0, 27),
        '…' => (1, 10),
        '.' => (1, 11),
        ':' => (1, 12),
        '(' => (1, 13),
        ')' => (1, 14),
        '-' => (1, 15),
        '\'' => (1, 16),
        '!' => (1, 17),
        '_' => (1, 18),
        '+' => (1, 19),
        '\\' => (1, 20),
        '/' => (1, 21),
        '[' => (1, 22),
        ']' => (1, 23),
        '^' => (1, 24),
        '&' => (1, 25),
        '%' => (1, 26),
        ',' => (1, 27),
        '=' => (1, 28),
        '$' => (1, 29),
        '#' => (1, 30),
        'å' => (2, 0),
        'ö' => (2, 1),
        'ä' => (2, 2),
        '?' => (2, 3),
        '*' => (2, 4),
        // Accented letters fall back to their base letter.
        'á' | 'à' | 'â' | 'ã' => (0, 0),
        'é' | 'ê' => (0, 4),
        'í' => (0, 8),
        'ó' | 'ô' | 'õ' => (0, 14),
        'ú' | 'ü' => (0, 20),
        'ç' => (0, 2),
        _ => (0, 30),
    }
}

/// Text in the skin's 5x6 bitmap font, clipped to `width` skin pixels, scrolled by `offset`.
fn text(p: &Painter, s: &str, x: f32, y: f32, width: f32, offset: f32) {
    let clip = p.rect(x, y, width, 6.0);
    let painter = p.ui.painter().with_clip_rect(clip);
    let Some((tex, size)) = p.skin.sheets.get("text") else { return };
    for (i, ch) in s.chars().enumerate() {
        let gx = x + i as f32 * 5.0 - offset;
        if gx + 5.0 < x || gx > x + width {
            continue;
        }
        let (row, col) = glyph(ch);
        let uv = Rect::from_min_size(pos2(col as f32 * 5.0 / size.x, row as f32 * 6.0 / size.y), vec2(5.0 / size.x, 6.0 / size.y));
        painter.image(tex.id(), p.rect(gx, y, 5.0, 6.0), uv, Color32::WHITE);
    }
}

/// Draws the main window filling `ui` and returns what the user did.
pub fn draw(ui: &mut Ui, skin: &Skin, st: &State) -> Vec<Event> {
    let full = ui.max_rect();
    let s = (full.width() / W).min(full.height() / H).max(1.0);
    let p = Painter { ui, skin, origin: full.min, s };
    let mut ev = vec![];
    let down = ui.input(|i| i.pointer.primary_down());

    p.sprite("main", [0.0, 0.0, W, H], 0.0, 0.0);
    p.sprite("titlebar", [27.0, 0.0, W, 14.0], 0.0, 0.0);

    // Clickable sprite: draws the pressed variant while held, reports a click on release.
    let button = |id: &str, x: f32, y: f32, w: f32, h: f32| {
        let r = p.rect(x, y, w, h);
        let resp = ui.interact(r, ui.id().with(id), Sense::click());
        (resp.is_pointer_button_down_on(), resp.clicked())
    };

    // Title bar: drag to move, minimize / close buttons.
    let title = ui.interact(p.rect(0.0, 0.0, 240.0, 14.0), ui.id().with("wa-title"), Sense::drag());
    crate::ui::drag_window(ui, &title);
    for (id, x, src, pressed_src, e) in [
        ("wa-min", 244.0, [9.0, 0.0], [9.0, 9.0], Event::Minimize),
        ("wa-close", 264.0, [18.0, 0.0], [18.0, 9.0], Event::Exit),
    ] {
        let (held, clicked) = button(id, x, 3.0, 9.0, 9.0);
        let src = if held { pressed_src } else { src };
        p.sprite("titlebar", [src[0], src[1], 9.0, 9.0], x, 3.0);
        if clicked {
            ev.push(e);
        }
    }

    // Transport buttons.
    for (id, x, sx, w, h, e) in [
        ("wa-prev", 16.0, 0.0, 23.0, 18.0, Event::Prev),
        ("wa-play", 39.0, 23.0, 23.0, 18.0, Event::Play),
        ("wa-pause", 62.0, 46.0, 23.0, 18.0, Event::Pause),
        ("wa-stop", 85.0, 69.0, 23.0, 18.0, Event::Stop),
        ("wa-next", 108.0, 92.0, 22.0, 18.0, Event::Next),
    ] {
        let (held, clicked) = button(id, x, 88.0, w, h);
        p.sprite("cbuttons", [sx, if held { 18.0 } else { 0.0 }, w, h], x, 88.0);
        if clicked {
            ev.push(e);
        }
    }
    let (held, clicked) = button("wa-eject", 136.0, 89.0, 22.0, 16.0);
    p.sprite("cbuttons", [114.0, if held { 16.0 } else { 0.0 }, 22.0, 16.0], 136.0, 89.0);
    if clicked {
        ev.push(Event::Exit);
    }

    // Shuffle / repeat toggles: rows are normal, pressed, active, active+pressed.
    for (id, x, sx, w, active, e) in [("wa-shuf", 164.0, 28.0, 47.0, st.shuffle, Event::Shuffle), ("wa-rep", 210.0, 0.0, 28.0, st.repeat, Event::Repeat)] {
        let (held, clicked) = button(id, x, 89.0, w, 15.0);
        let row = (active as u8 * 2 + held as u8) as f32 * 15.0;
        p.sprite("shufrep", [sx, row, w, 15.0], x, 89.0);
        if clicked {
            ev.push(e);
        }
    }

    // Play state, mono/stereo.
    let state_x = if st.playing && !st.paused { 0.0 } else if st.playing { 9.0 } else { 18.0 };
    p.sprite("playpaus", [state_x, 0.0, 9.0, 9.0], 26.0, 28.0);
    let on = st.playing;
    p.sprite("monoster", [29.0, if on && !st.stereo { 0.0 } else { 12.0 }, 27.0, 12.0], 212.0, 41.0);
    p.sprite("monoster", [0.0, if on && st.stereo { 0.0 } else { 12.0 }, 29.0, 12.0], 239.0, 41.0);

    // Time m:ss in the big digits.
    if st.playing {
        let t = st.time.max(0.0) as u64;
        let (m, sec) = ((t / 60).min(99), t % 60);
        for (d, x) in [(m / 10, 48.0), (m % 10, 60.0), (sec / 10, 78.0), (sec % 10, 90.0)] {
            p.sprite("numbers", [d as f32 * 9.0, 0.0, 9.0, 13.0], x, 26.0);
        }
        text(&p, &format!("{:>3}", st.kbps.min(999)), 111.0, 43.0, 15.0, 0.0);
        text(&p, &format!("{:>2}", st.khz.min(99)), 156.0, 43.0, 10.0, 0.0);
    }

    // Scrolling title.
    let label = format!("{} *** ", st.title);
    let width_px = label.chars().count() as f32 * 5.0;
    let fits = st.title.chars().count() as f32 * 5.0 <= 154.0;
    let offset = if fits { 0.0 } else { (ui.input(|i| i.time) as f32 * 24.0) % width_px };
    let shown = if fits { st.title.to_string() } else { format!("{label}{label}") };
    text(&p, &shown, 111.0, 27.0, 154.0, offset);
    if !fits {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
    }

    // Spectrum analyser: 19 bars of 3 px, colours 2 (top) .. 17 (bottom) from viscolor.
    p.fill(24.0, 43.0, 76.0, 16.0, skin.vis[0]);
    for (i, b) in st.bars.iter().enumerate() {
        let h = (b.clamp(0.0, 1.0) * 16.0).round() as i32;
        for row in 0..h {
            let col = skin.vis[(17 - row.min(15)) as usize];
            p.fill(24.0 + i as f32 * 4.0, 43.0 + 15.0 - row as f32, 3.0, 1.0, col);
        }
    }

    // Sliders: position, volume, balance.
    let slider = |id: &str, x: f32, y: f32, w: f32, h: f32| {
        let r = p.rect(x, y, w, h);
        let resp = ui.interact(r, ui.id().with(id), Sense::click_and_drag());
        let f = resp.interact_pointer_pos().map(|pp| ((pp.x - r.left()) / r.width()).clamp(0.0, 1.0));
        (resp, f)
    };
    p.sprite("posbar", [0.0, 0.0, 248.0, 10.0], 16.0, 72.0);
    let (seek, f) = slider("wa-seek", 16.0, 72.0, 248.0, 10.0);
    if st.dur > 0.0 {
        let frac = f.unwrap_or((st.time / st.dur) as f32);
        p.sprite("posbar", [if seek.is_pointer_button_down_on() { 278.0 } else { 248.0 }, 0.0, 29.0, 10.0], 16.0 + frac * (248.0 - 29.0), 72.0);
        if seek.drag_stopped() || seek.clicked() {
            ev.push(Event::Seek(frac));
        }
    }

    let (vol, f) = slider("wa-vol", 107.0, 57.0, 68.0, 13.0);
    let v = f.unwrap_or(st.volume);
    p.sprite("volume", [0.0, (v * 27.0).round() * 15.0, 68.0, 13.0], 107.0, 57.0);
    p.sprite("volume", [if vol.is_pointer_button_down_on() { 0.0 } else { 15.0 }, 422.0, 14.0, 11.0], 107.0 + v * (68.0 - 14.0), 58.0);
    if f.is_some() && down {
        ev.push(Event::Volume(v));
    }

    let (bal, f) = slider("wa-bal", 177.0, 57.0, 38.0, 13.0);
    let b = f.map(|f| f * 2.0 - 1.0).unwrap_or(st.balance);
    // Snap to centre near the middle, like Winamp.
    let b = if b.abs() < 0.08 { 0.0 } else { b };
    p.sprite("balance", [9.0, (b.abs() * 27.0).round() * 15.0, 38.0, 13.0], 177.0, 57.0);
    p.sprite("balance", [if bal.is_pointer_button_down_on() { 0.0 } else { 15.0 }, 422.0, 14.0, 11.0], 177.0 + (b + 1.0) / 2.0 * (38.0 - 14.0), 58.0);
    if f.is_some() && (bal.drag_stopped() || bal.clicked()) {
        ev.push(Event::Balance(b));
    }
    ev
}

/// 19 log-spaced band levels (0..1) from mono samples, with Winamp-style falloff.
pub fn analyse(pcm: &[f32], bars: &mut [f32; 19]) {
    let n = pcm.len().min(512);
    if n < 64 {
        for b in bars.iter_mut() {
            *b = (*b - 0.04).max(0.0);
        }
        return;
    }
    let x = &pcm[pcm.len() - n..];
    for (i, bar) in bars.iter_mut().enumerate() {
        // Bin range for band i on a log scale from bin 1 to n/2.
        let lo = (n as f32 / 2.0).powf(i as f32 / 19.0).max(1.0) as usize;
        let hi = ((n as f32 / 2.0).powf((i + 1) as f32 / 19.0) as usize).max(lo + 1);
        let mut peak = 0.0f32;
        for k in lo..hi.min(n / 2) {
            // Goertzel: one bin's magnitude without per-sample trig.
            let coeff = 2.0 * (std::f32::consts::TAU * k as f32 / n as f32).cos();
            let (mut s1, mut s2) = (0.0f32, 0.0f32);
            for v in x {
                let s0 = v + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            let power = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0);
            peak = peak.max(power.sqrt() / n as f32);
        }
        let level = ((peak.max(1e-6).log10() + 3.2) / 2.6).clamp(0.0, 1.0);
        *bar = if level > *bar { level } else { (*bar - 0.04).max(level) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themed_ramp() {
        let p = crate::theme::c();
        let ramp = Ramp::new(&p);
        // Black and white land on the palette's ends; the display's green takes the accent.
        assert_eq!(ramp.map(Color32::BLACK), ramp.stops[0]);
        assert_eq!(ramp.map(Color32::WHITE), *ramp.stops.last().unwrap());
        assert_eq!(ramp.map(Color32::from_rgb(0, 232, 0)), ramp.stops[0].lerp_to_gamma(p.accent, 1.0));
    }

    #[test]
    fn viscolor_and_glyphs() {
        let v = parse_viscolor("// comment\n10,20,30, // bg\n1,2,3\n");
        assert_eq!(v[0], Color32::from_rgb(10, 20, 30));
        assert_eq!(v[1], Color32::from_rgb(1, 2, 3));
        assert_eq!(v[2], Color32::from_rgb(239, 49, 16));
        assert_eq!(glyph('A'), (0, 0));
        assert_eq!(glyph('9'), (1, 9));
        assert_eq!(glyph('ã'), (0, 0));
        assert_eq!(glyph('~'), (0, 30));
    }

    #[test]
    fn analyser_finds_the_tone() {
        // A low tone should light the low bands more than the top ones.
        let pcm: Vec<f32> = (0..512).map(|t| (std::f32::consts::TAU * 4.0 * t as f32 / 512.0).sin() * 0.8).collect();
        let mut bars = [0.0; 19];
        analyse(&pcm, &mut bars);
        assert!(bars[..6].iter().cloned().fold(0.0, f32::max) > bars[15..].iter().cloned().fold(0.0, f32::max) + 0.2, "{bars:?}");
    }
}

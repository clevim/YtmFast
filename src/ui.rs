//! Painted icons and the custom widgets the app needs. Colours come from `theme::c()`.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, FontFamily, FontId, Galley, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui, pos2, vec2,
    text::{LayoutJob, TextFormat, TextWrapping},
};
use rustypipe::model::{MusicPlaylistItem, Thumbnail, TrackItem};

use crate::theme::{c, look};
use crate::i18n::tr;

pub const ROW_H: f32 = 56.0;
pub const ROW_H_COMPACT: f32 = 36.0;

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

// ponytail: system fonts by path, Roboto (what YTM uses) first; egui's built-in font is the fallback
/// Fallbacks for what the text fonts lack: "fancy text" letters (𝐁𝐋𝐗, from the math alphabets)
/// and symbols people put in names. Only loaded when installed.
const FALLBACK_FONTS: [(&str, &[&str]); 3] = [
    ("math", &["/usr/share/fonts/noto/NotoSansMath-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSansMath-Regular.ttf"]),
    ("symbols2", &["/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf", "C:\\Windows\\Fonts\\seguisym.ttf", "/System/Library/Fonts/Apple Symbols.ttf"]),
    ("symbols", &["/usr/share/fonts/noto/NotoSansSymbols-Regular.ttf", "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf"]),
];

const FONTS: [(&str, &[&str]); 3] = [
    ("regular", &[
        "/usr/share/fonts/TTF/Roboto-Regular.ttf",
        "/usr/share/fonts/truetype/roboto/unhinted/RobotoTTF/Roboto-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ]),
    ("medium", &[
        "/usr/share/fonts/TTF/Roboto-Medium.ttf",
        "/usr/share/fonts/truetype/roboto/unhinted/RobotoTTF/Roboto-Medium.ttf",
        "/usr/share/fonts/noto/NotoSans-Medium.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Medium.ttf",
        "C:\\Windows\\Fonts\\seguisb.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    ]),
    ("bold", &[
        "/usr/share/fonts/TTF/Roboto-Bold.ttf",
        "/usr/share/fonts/truetype/roboto/unhinted/RobotoTTF/Roboto-Bold.ttf",
        "/usr/share/fonts/noto/NotoSans-Bold.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
        "C:\\Windows\\Fonts\\segoeuib.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    ]),
];

/// Fonts and text sizes; colours are applied separately by `theme::apply`.
pub fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let base = fonts.families[&FontFamily::Proportional].clone();
    let mut loaded: Vec<&str> = vec![];
    for &(name, paths) in FONTS.iter().chain(&FALLBACK_FONTS) {
        if let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) {
            fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
            loaded.push(name);
        }
    }
    let family = |prefs: &[&str]| {
        let mut f: Vec<String> = prefs.iter().filter(|n| loaded.contains(n)).take(1).map(|n| n.to_string()).collect();
        f.extend(base.iter().cloned());
        f.extend(FALLBACK_FONTS.iter().map(|(n, _)| n.to_string()).filter(|n| loaded.contains(&n.as_str())));
        f
    };
    fonts.families.insert(FontFamily::Proportional, family(&["regular"]));
    fonts.families.insert(FontFamily::Name("medium".into()), family(&["medium", "regular"]));
    fonts.families.insert(FontFamily::Name("bold".into()), family(&["bold", "medium", "regular"]));
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|s| {
        use egui::TextStyle::*;
        s.text_styles = [
            (Small, regular(12.0)),
            (Body, regular(14.0)),
            (Button, regular(14.0)),
            (Monospace, FontId::monospace(13.0)),
            (Heading, bold(22.0)),
        ]
        .into();
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.menu_margin = egui::Margin::same(6);
    });
}

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Search,
    Home,
    Queue,
    Heart,
    Volume,
    Mute,
    Person,
    Plus,
    Mini,
    Shuffle,
    Repeat,
    RepeatOne,
    Back,
    Forward,
    Gear,
    Lyrics,
    Wave,
    Radio,
    Video,
    Mic,
    Expand,
    Collapse,
    Close,
    Stop,
    Minimize,
}

fn tri(p: &egui::Painter, pts: [Pos2; 3], col: Color32) {
    p.add(Shape::convex_polygon(pts.to_vec(), col, Stroke::NONE));
}

fn arc(center: Pos2, r: f32, from: f32, to: f32) -> Vec<Pos2> {
    (0..=12)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / 12.0).to_radians();
            center + vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// Icons drawn on a 24-unit grid, scaled to `r`.
pub fn paint_icon(p: &egui::Painter, icon: Icon, r: Rect, col: Color32) {
    let u = r.width() / 24.0;
    let pt = |x: f32, y: f32| pos2(r.left() + x * u, r.top() + y * u);
    let bx = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pt(x0, y0), pt(x1, y1));
    let st = Stroke::new(2.0 * u, col);
    let line = |pts: Vec<Pos2>| {
        p.add(egui::epaint::PathShape::line(pts, st));
    };
    match icon {
        Icon::Play => tri(p, [pt(7.0, 4.5), pt(19.5, 12.0), pt(7.0, 19.5)], col),
        Icon::Pause => {
            p.rect_filled(bx(6.0, 5.0, 10.0, 19.0), 1.0, col);
            p.rect_filled(bx(14.0, 5.0, 18.0, 19.0), 1.0, col);
        }
        Icon::Next => {
            tri(p, [pt(5.5, 6.0), pt(15.5, 12.0), pt(5.5, 18.0)], col);
            p.rect_filled(bx(16.0, 6.0, 18.5, 18.0), 0.0, col);
        }
        Icon::Prev => {
            tri(p, [pt(18.5, 6.0), pt(8.5, 12.0), pt(18.5, 18.0)], col);
            p.rect_filled(bx(5.5, 6.0, 8.0, 18.0), 0.0, col);
        }
        Icon::Search => {
            p.circle_stroke(pt(10.5, 10.5), 6.0 * u, st);
            p.line_segment([pt(15.0, 15.0), pt(20.5, 20.5)], st);
        }
        Icon::Home => {
            tri(p, [pt(12.0, 3.0), pt(21.5, 11.5), pt(2.5, 11.5)], col);
            p.rect_filled(bx(5.5, 11.0, 18.5, 20.5), 0.0, col);
        }
        Icon::Queue => {
            p.line_segment([pt(3.0, 6.0), pt(21.0, 6.0)], st);
            p.line_segment([pt(3.0, 11.0), pt(21.0, 11.0)], st);
            p.line_segment([pt(3.0, 16.0), pt(12.0, 16.0)], st);
            tri(p, [pt(15.0, 13.5), pt(21.0, 17.0), pt(15.0, 20.5)], col);
        }
        Icon::Heart => {
            p.circle_filled(pt(8.0, 9.0), 4.6 * u, col);
            p.circle_filled(pt(16.0, 9.0), 4.6 * u, col);
            p.add(Shape::convex_polygon(vec![pt(3.6, 10.8), pt(12.0, 7.5), pt(20.4, 10.8), pt(12.0, 20.5)], col, Stroke::NONE));
        }
        Icon::Volume | Icon::Mute => {
            p.rect_filled(bx(3.0, 9.0, 7.0, 15.0), 0.0, col);
            p.add(Shape::convex_polygon(vec![pt(7.0, 9.0), pt(12.5, 4.5), pt(12.5, 19.5), pt(7.0, 15.0)], col, Stroke::NONE));
            if matches!(icon, Icon::Mute) {
                p.line_segment([pt(15.5, 9.0), pt(21.5, 15.0)], st);
                p.line_segment([pt(21.5, 9.0), pt(15.5, 15.0)], st);
            } else {
                line(arc(pt(12.5, 12.0), 4.0 * u, -45.0, 45.0));
                line(arc(pt(12.5, 12.0), 7.5 * u, -50.0, 50.0));
            }
        }
        Icon::Person => {
            p.circle_filled(pt(12.0, 8.0), 4.0 * u, col);
            p.add(Shape::convex_polygon(arc(pt(12.0, 21.0), 8.0 * u, 180.0, 360.0), col, Stroke::NONE));
        }
        Icon::Plus => {
            p.line_segment([pt(12.0, 4.0), pt(12.0, 20.0)], st);
            p.line_segment([pt(4.0, 12.0), pt(20.0, 12.0)], st);
        }
        Icon::Mini => {
            p.rect_stroke(bx(3.0, 5.0, 21.0, 19.0), 2.0, Stroke::new(1.6 * u, col), StrokeKind::Inside);
            p.rect_filled(bx(12.0, 11.0, 19.0, 17.0), 1.0, col);
        }
        Icon::Shuffle => {
            line(vec![pt(3.0, 7.0), pt(7.0, 7.0), pt(15.0, 17.0), pt(19.0, 17.0)]);
            line(vec![pt(3.0, 17.0), pt(7.0, 17.0), pt(15.0, 7.0), pt(19.0, 7.0)]);
            tri(p, [pt(18.0, 4.0), pt(22.0, 7.0), pt(18.0, 10.0)], col);
            tri(p, [pt(18.0, 14.0), pt(22.0, 17.0), pt(18.0, 20.0)], col);
        }
        Icon::Repeat | Icon::RepeatOne => {
            line(vec![pt(5.0, 13.0), pt(5.0, 8.0), pt(17.0, 8.0)]);
            line(vec![pt(19.0, 11.0), pt(19.0, 16.0), pt(7.0, 16.0)]);
            tri(p, [pt(16.0, 4.5), pt(20.5, 8.0), pt(16.0, 11.5)], col);
            tri(p, [pt(8.0, 12.5), pt(3.5, 16.0), pt(8.0, 19.5)], col);
            if matches!(icon, Icon::RepeatOne) {
                p.text(pt(12.0, 12.0), egui::Align2::CENTER_CENTER, "1", bold(9.0 * u), col);
            }
        }
        Icon::Back | Icon::Forward => {
            let s = if matches!(icon, Icon::Back) { 1.0 } else { -1.0 };
            let x = |v: f32| 12.0 - s * (12.0 - v);
            line(vec![pt(x(15.0), 5.0), pt(x(8.0), 12.0), pt(x(15.0), 19.0)]);
        }
        Icon::Gear => {
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                let d = vec2(a.cos(), a.sin());
                p.line_segment([pt(12.0, 12.0) + d * 6.0 * u, pt(12.0, 12.0) + d * 9.5 * u], Stroke::new(3.0 * u, col));
            }
            p.circle_stroke(pt(12.0, 12.0), 5.5 * u, st);
        }
        Icon::Lyrics => {
            p.rect_stroke(bx(4.0, 4.0, 20.0, 17.0), 2.0, Stroke::new(1.8 * u, col), StrokeKind::Inside);
            tri(p, [pt(7.0, 16.0), pt(12.0, 16.0), pt(7.0, 21.0)], col);
            p.line_segment([pt(8.0, 8.5), pt(16.0, 8.5)], st);
            p.line_segment([pt(8.0, 12.5), pt(13.0, 12.5)], st);
        }
        Icon::Wave => {
            for (i, h) in [4.0, 9.0, 14.0, 9.0, 5.0].into_iter().enumerate() {
                let x = 4.0 + i as f32 * 4.0;
                p.line_segment([pt(x, 12.0 - h / 2.0), pt(x, 12.0 + h / 2.0)], Stroke::new(2.2 * u, col));
            }
        }
        Icon::Video => {
            p.rect_stroke(bx(2.5, 5.0, 21.5, 19.0), 3.0 * u, Stroke::new(1.8 * u, col), StrokeKind::Inside);
            tri(p, [pt(10.0, 8.5), pt(15.5, 12.0), pt(10.0, 15.5)], col);
        }
        Icon::Radio => {
            p.circle_filled(pt(12.0, 12.0), 2.2 * u, col);
            line(arc(pt(12.0, 12.0), 5.5 * u, -40.0, 40.0));
            line(arc(pt(12.0, 12.0), 5.5 * u, 140.0, 220.0));
            line(arc(pt(12.0, 12.0), 9.0 * u, -40.0, 40.0));
            line(arc(pt(12.0, 12.0), 9.0 * u, 140.0, 220.0));
        }
        Icon::Mic => {
            p.rect_stroke(bx(9.0, 3.0, 15.0, 14.0), 3.0 * u, st, StrokeKind::Inside);
            line(arc(pt(12.0, 10.5), 6.0 * u, 0.0, 180.0));
            p.line_segment([pt(12.0, 16.5), pt(12.0, 21.0)], st);
            p.line_segment([pt(8.5, 21.0), pt(15.5, 21.0)], st);
        }
        // Arrows pointing out of (Expand) or into (Collapse) the four corners.
        Icon::Expand | Icon::Collapse => {
            for (cx, cy) in [(4.0, 4.0), (20.0, 4.0), (4.0, 20.0), (20.0, 20.0)] {
                let (dx, dy) = (if cx < 12.0 { 1.0 } else { -1.0 }, if cy < 12.0 { 1.0 } else { -1.0 });
                // Corner of the "L": on the edge for Expand, 5 units in for Collapse.
                let k = if icon == Icon::Expand { 0.0 } else { 5.0 };
                let (x, y) = (cx + dx * k, cy + dy * k);
                let s = if icon == Icon::Expand { 1.0 } else { -1.0 };
                line(vec![pt(x + dx * 5.0 * s, y), pt(x, y), pt(x, y + dy * 5.0 * s)]);
            }
        }
        Icon::Stop => {
            p.rect_filled(bx(6.5, 6.5, 17.5, 17.5), 1.5 * u, col);
        }
        Icon::Minimize => {
            p.line_segment([pt(6.0, 17.0), pt(18.0, 17.0)], st);
        }
        Icon::Close => {
            p.line_segment([pt(6.0, 6.0), pt(18.0, 18.0)], st);
            p.line_segment([pt(18.0, 6.0), pt(6.0, 18.0)], st);
        }
    }
}

/// The app logo (same geometry as packaging/linux/ytmfast.svg), drawn as vectors in `r`.
pub fn paint_logo(p: &egui::Painter, r: Rect) {
    let c = r.center();
    let rad = r.width().min(r.height()) / 2.0;
    p.circle_filled(c, rad, Color32::from_rgb(0xED, 0x1C, 0x1C));
    p.circle_stroke(c, rad * 0.5977, Stroke::new(rad * 0.0953, Color32::WHITE));
    // Triangle shrunk toward its incentre and stroked round: rounded corners, original size.
    let tri = [(-0.164, -0.2452), (0.2496, 0.0), (-0.164, 0.2452)].map(|(x, y)| c + vec2(x, y) * rad);
    p.add(egui::epaint::PathShape::convex_polygon(tri.to_vec(), Color32::WHITE, Stroke::new(rad * 0.0625, Color32::WHITE)));
}

/// Round icon button. `active` tints it with the accent (shuffle/repeat on).
pub fn icon_toggle(ui: &mut Ui, icon: Icon, size: f32, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let p = c();
    if resp.hovered() {
        ui.painter().circle_filled(rect.center(), size / 2.0, p.wash());
    }
    let col = if active { p.accent } else { p.text };
    paint_icon(ui.painter(), icon, rect.shrink(size * 0.22), col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32) -> Response {
    icon_toggle(ui, icon, size, false)
}

/// Heart toggle: filled when liked, outlined otherwise.
pub fn heart_button(ui: &mut Ui, liked: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
    let p = c();
    let painter = ui.painter();
    let bg = if resp.hovered() {
        painter.circle_filled(rect.center(), 18.0, p.wash());
        // what the wash looks like over the bar, so the cut-out matches
        p.bar.lerp_to_gamma(p.text, 0.09)
    } else {
        p.bar
    };
    let r = rect.shrink(9.0);
    if liked {
        paint_icon(painter, Icon::Heart, r, p.text);
    } else {
        paint_icon(painter, Icon::Heart, r, p.text2);
        paint_icon(painter, Icon::Heart, r.shrink(2.2), bg);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Rounded YTM-style button: filled (primary) or outlined.
pub fn pill(ui: &mut Ui, icon: Option<Icon>, text: &str, primary: bool) -> Response {
    let p = c();
    let fg = if primary { p.bg } else { p.text };
    let galley = ui.painter().layout_no_wrap(text.into(), medium(14.0), fg);
    let icon_w = if icon.is_some() { 26.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 36.0, 36.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (fill, stroke) = match (primary, resp.hovered()) {
        (true, false) => (p.text, Stroke::NONE),
        (true, true) => (p.text.gamma_multiply(0.85), Stroke::NONE),
        (false, false) => (Color32::TRANSPARENT, Stroke::new(1.0, p.text2.gamma_multiply(0.5))),
        (false, true) => (p.wash(), Stroke::new(1.0, p.text2.gamma_multiply(0.7))),
    };
    let painter = ui.painter();
    painter.rect(rect, 18.0, fill, stroke, StrokeKind::Inside);
    let mut x = rect.left() + 18.0;
    if let Some(i) = icon {
        paint_icon(painter, i, Rect::from_center_size(pos2(x + 9.0, rect.center().y), vec2(20.0, 20.0)), fg);
        x += icon_w;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// On/off switch with a label and an optional hint underneath. Returns true when toggled.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, hint: &str) -> bool {
    let p = c();
    let w = ui.available_width();
    let h = if hint.is_empty() { 40.0 } else { 54.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, if look().motion { 0.15 } else { 0.0 });
    let painter = ui.painter();
    let x = rect.left();
    painter.galley(pos2(x, rect.top() + 8.0), line(ui, label, medium(14.0), p.text, w - 70.0), p.text);
    if !hint.is_empty() {
        painter.galley(pos2(x, rect.top() + 28.0), line(ui, hint, regular(12.5), p.text2, w - 70.0), p.text2);
    }
    let track = Rect::from_center_size(pos2(rect.right() - 22.0, rect.top() + 20.0), vec2(36.0, 20.0));
    let off = p.text2.gamma_multiply(0.5);
    painter.rect_filled(track, 10.0, off.lerp_to_gamma(p.accent, t));
    let knob = pos2(egui::lerp(track.left() + 10.0..=track.right() - 10.0, t), track.center().y);
    painter.circle_filled(knob, 7.5, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Single line of text, truncated with an ellipsis at `max_w`.
pub fn line(ui: &Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap = TextWrapping::truncate_at_width(max_w.max(1.0));
    ui.painter().layout_job(job)
}

/// Picks the smallest thumbnail covering `px`; Google image URLs get resized server side.
fn img(t: &[Thumbnail], px: u32) -> Option<(String, Rect)> {
    let th = t.iter().find(|t| t.width >= px).or(t.last())?;
    Some(img_src(&th.url, th.width, th.height, px))
}

fn img_src(url: &str, width: u32, height: u32, px: u32) -> (String, Rect) {
    let full = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    // Google's image server is asked for a px x px square and crops it itself (`-p`; without it the
    // picture is only fitted inside the box, and a non-square one came back stretched).
    if let Some(i) = google_size(url) {
        return (format!("{}=w{px}-h{px}-p-l90-rj", &url[..i]), full);
    }
    // Anything else (video thumbnails) is drawn square: a centred crop at its exact ratio.
    let (w, mut h) = (width.max(1) as f32, height.max(1) as f32);
    // YouTube's 4:3 thumbnails (hqdefault, sddefault) are 16:9 pictures with black bars: skip them.
    let bars = url.contains("i.ytimg.com") && width * 3 == height * 4;
    if bars {
        h = w * 9.0 / 16.0;
    }
    let uv = if w > h {
        let k = h / w;
        Rect::from_min_max(pos2((1.0 - k) / 2.0, 0.0), pos2((1.0 + k) / 2.0, 1.0))
    } else if h > w {
        let k = w / h;
        Rect::from_min_max(pos2(0.0, (1.0 - k) / 2.0), pos2(1.0, (1.0 + k) / 2.0))
    } else {
        full
    };
    // The picture sits in the middle 3/4 of the image's height.
    let uv = if bars { Rect::from_min_max(pos2(uv.min.x, 0.125), pos2(uv.max.x, 0.875)) } else { uv };
    (url.to_owned(), uv)
}

/// Where a Google image URL's size suffix (`=w60-h60-…`, `=s192`) starts, if it has one.
fn google_size(url: &str) -> Option<usize> {
    let host = url.split('/').nth(2)?;
    if !(host.ends_with("googleusercontent.com") || host.ends_with("ggpht.com")) {
        return None;
    }
    let i = url.rfind('=')?;
    let mut rest = url[i + 1..].chars();
    (matches!(rest.next(), Some('w' | 's')) && rest.next().is_some_and(|c| c.is_ascii_digit())).then_some(i)
}

/// The URL `paint_cover` will request for a cover drawn `size` points wide.
pub fn cover_url(ctx: &egui::Context, t: &[Thumbnail], size: f32) -> Option<String> {
    img(t, (size * ctx.pixels_per_point()).ceil() as u32).map(|(u, _)| u)
}

thread_local! {
    /// Image URL -> when it was last drawn (egui time), to know which decoded images are stale.
    static SEEN: std::cell::RefCell<std::collections::HashMap<String, f64>> = Default::default();
}

fn mark_seen(ctx: &egui::Context, url: &str) {
    let now = ctx.input(|i| i.time);
    SEEN.with(|s| {
        let mut s = s.borrow_mut();
        match s.get_mut(url) {
            Some(t) => *t = now,
            None => drop(s.insert(url.to_owned(), now)),
        }
    });
}

/// The window (and its egui clock) is gone: what was seen belongs to it.
pub fn clear_seen() {
    SEEN.with(|s| s.borrow_mut().clear());
}

/// Decoded artwork (RGBA, the big part of image memory): anything not drawn for two minutes goes,
/// and past `budget` the least recently drawn go too. They come back from the disk cache if shown.
pub fn forget_stale_images(ctx: &egui::Context, budget: usize) {
    let now = ctx.input(|i| i.time);
    let old: Vec<String> = SEEN.with(|s| s.borrow().iter().filter(|(_, t)| now - **t > 120.0).map(|(u, _)| u.clone()).collect());
    for url in &old {
        ctx.forget_image(url);
        SEEN.with(|s| s.borrow_mut().remove(url));
    }
    let held: usize = ctx.loaders().image.lock().iter().map(|l| l.byte_size()).sum();
    if held <= budget {
        return;
    }
    let mut stale: Vec<(f64, String)> = SEEN.with(|s| s.borrow().iter().filter(|(_, t)| now - **t > 20.0).map(|(u, t)| (*t, u.clone())).collect());
    stale.sort_by(|a, b| a.0.total_cmp(&b.0));
    // ponytail: assumes images are similar in size; exact per-image sizes aren't exposed by egui
    let per = held / SEEN.with(|s| s.borrow().len()).max(1);
    let mut over = held - budget;
    for (_, url) in stale {
        ctx.forget_image(&url);
        SEEN.with(|s| s.borrow_mut().remove(&url));
        over = over.saturating_sub(per.max(1));
        if over == 0 {
            break;
        }
    }
}

pub fn paint_cover(ui: &Ui, t: &[Thumbnail], rect: Rect, radius: u8) {
    ui.painter().rect_filled(rect, radius, c().selected);
    let px = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    if let Some((url, uv)) = img(t, px) {
        mark_seen(ui.ctx(), &url);
        egui::Image::new(url).uv(uv).corner_radius(radius).show_loading_spinner(false).paint_at(ui, rect);
    }
}

/// Wide image cropped to fill `rect` (artist header), fading into the background at the bottom.
pub fn paint_banner(ui: &Ui, t: &[Thumbnail], rect: Rect) {
    let px = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    let Some(th) = t.iter().find(|t| t.width >= px).or(t.last()) else { return };
    // Google's server sends the whole original at the rect's width (the largest size YouTube lists
    // is 1080 wide, blurry on a wide window). Its proportions differ from the listed sizes, so the
    // crop below waits for the real texture size.
    let url = google_size(&th.url).map_or_else(|| th.url.clone(), |i| format!("{}=w{px}-l90-rj", &th.url[..i]));
    mark_seen(ui.ctx(), &url);
    let Ok(egui::load::TexturePoll::Ready { texture }) = egui::Image::new(url).load_for_size(ui.ctx(), rect.size()) else { return };
    let (img_aspect, rect_aspect) = (texture.size.x / texture.size.y.max(1.0), rect.width() / rect.height());
    // Crop the image to the rect's aspect around a third of the way down (where faces usually are,
    // as YouTube's own crop does).
    let uv = if img_aspect > rect_aspect {
        let w = rect_aspect / img_aspect;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        let h = img_aspect / rect_aspect;
        let top = (0.3 - h / 2.0).clamp(0.0, 1.0 - h);
        Rect::from_min_max(pos2(0.0, top), pos2(1.0, top + h))
    };
    egui::Image::from_texture(texture).uv(uv).paint_at(ui, rect);
    gradient(ui, rect, Color32::TRANSPARENT, c().bg);
}

/// Vertical gradient from `top` to `bottom` over `rect`.
pub fn gradient(ui: &Ui, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    ui.painter().add(Shape::mesh(mesh));
}

fn ease(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// Cover drawn as a textured plane rotated in 3D (`rot` = pitch, yaw in radians) with perspective,
/// scaled by `scale` around its center. `rect` must stay fixed across frames: it picks the image URL,
/// and a changing URL means a fresh download every frame (that was the hover flicker).
fn paint_cover_3d(ui: &Ui, t: &[Thumbnail], rect: Rect, radius: u8, rot: egui::Vec2, scale: f32) {
    let px = (rect.width() * ui.ctx().pixels_per_point()).ceil() as u32;
    let tex = img(t, px).and_then(|(url, uv)| {
        mark_seen(ui.ctx(), &url);
        match egui::Image::new(url).load_for_size(ui.ctx(), rect.size()) {
            Ok(egui::load::TexturePoll::Ready { texture }) => Some((texture.id, uv)),
            _ => None,
        }
    });
    let Some((tex, uv)) = tex.filter(|_| rot.length() > 0.002 || scale > 1.001) else {
        return paint_cover(ui, t, rect, radius);
    };
    // A radius of half the size or more means a round avatar.
    let round = f32::from(radius) * 2.0 >= rect.width() - 1.0;
    let (w, h) = (rect.width(), rect.height());
    let d = w * 2.5;
    let (sy, cy) = rot.y.sin_cos();
    let (sx, cx) = rot.x.sin_cos();
    let mut mesh = egui::Mesh::with_texture(tex);
    // (u, v) in 0..1 over the cover -> perspective-projected vertex.
    let mut vertex = |u: f32, v: f32| {
        let (x, y) = ((u - 0.5) * w, (v - 0.5) * h);
        let (x1, z1) = (x * cy, -x * sy);
        let (y2, z2) = (y * cx - z1 * sx, y * sx + z1 * cx);
        let s = scale * d / (d + z2);
        // Parts tilted away get slightly darker: cheap fake lighting.
        let shade = 255.0 - (z2 / w * 160.0).clamp(0.0, 70.0);
        mesh.vertices.push(egui::epaint::Vertex {
            pos: rect.center() + vec2(x1 * s, y2 * s),
            uv: pos2(uv.min.x + u * uv.width(), uv.min.y + v * uv.height()),
            color: Color32::from_gray(shade as u8),
        });
    };
    if round {
        // Round avatars: rings around the centre, so the outline stays a circle under perspective.
        const RINGS: u32 = 6;
        const SEG: u32 = 48;
        vertex(0.5, 0.5);
        for r in 1..=RINGS {
            let rad = 0.5 * r as f32 / RINGS as f32;
            for i in 0..SEG {
                let a = i as f32 / SEG as f32 * std::f32::consts::TAU;
                vertex(0.5 + rad * a.cos(), 0.5 + rad * a.sin());
            }
        }
        let at = |r: u32, i: u32| 1 + (r - 1) * SEG + i % SEG;
        for i in 0..SEG {
            mesh.add_triangle(0, at(1, i), at(1, i + 1));
        }
        for r in 1..RINGS {
            for i in 0..SEG {
                mesh.add_triangle(at(r, i), at(r + 1, i), at(r + 1, i + 1));
                mesh.add_triangle(at(r, i), at(r + 1, i + 1), at(r, i + 1));
            }
        }
    } else {
        // A subdivided grid keeps the texture straight under perspective (two triangles would warp).
        const N: u32 = 8;
        for j in 0..=N {
            for i in 0..=N {
                vertex(i as f32 / N as f32, j as f32 / N as f32);
            }
        }
        for j in 0..N {
            for i in 0..N {
                let a = j * (N + 1) + i;
                mesh.add_triangle(a, a + 1, a + N + 2);
                mesh.add_triangle(a, a + N + 2, a + N + 1);
            }
        }
    }
    ui.painter().add(Shape::mesh(mesh));
}

/// Hover-reactive cover: lifts, casts a shadow and tilts toward the pointer. `k` = max tilt.
pub fn tilt_cover(ui: &Ui, resp: &Response, rect: Rect, t: &[Thumbnail], radius: u8, k: f32) -> f32 {
    let lk = look();
    let k = k * lk.tilt;
    let time = if lk.motion { 1.0 } else { 0.0 };
    let ctx = ui.ctx();
    let id = resp.id;
    let hov = ctx.animate_bool_with_time(id.with("h"), resp.hovered(), 0.2 * time);
    if k <= 0.0 {
        paint_cover(ui, t, rect, radius);
        return hov;
    }
    let target = resp.hover_pos().map_or(egui::Vec2::ZERO, |p| {
        ((p - rect.center()) / (rect.width() / 2.0)).clamp(vec2(-1.0, -1.0), vec2(1.0, 1.0))
    });
    let tx = ctx.animate_value_with_time(id.with("tx"), target.x, 0.15 * time);
    let ty = ctx.animate_value_with_time(id.with("ty"), target.y, 0.15 * time);
    let scale = 1.0 + 0.04 * hov;
    if hov > 0.01 {
        let shadow = egui::Shadow { offset: [0, 12], blur: 28, spread: 0, color: Color32::from_black_alpha((170.0 * hov) as u8) };
        ui.painter().add(shadow.as_shape(Rect::from_center_size(rect.center(), rect.size() * scale), radius));
    }
    paint_cover_3d(ui, t, rect, radius, vec2(ty, -tx) * k * hov, scale);
    hov
}

pub fn artists(t: &TrackItem) -> String {
    t.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
}

pub fn fmt_time(s: f64) -> String {
    let s = s.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn eq_bars(p: &egui::Painter, center: Pos2, col: Color32) {
    for (i, h) in [8.0, 14.0, 10.0].into_iter().enumerate() {
        let x = center.x - 6.0 + i as f32 * 5.0;
        p.rect_filled(Rect::from_min_max(pos2(x - 1.5, center.y + 7.0 - h), pos2(x + 1.5, center.y + 7.0)), 1.0, col);
    }
}

/// Animated hover background, shared by every clickable row.
fn hover_bg(ui: &Ui, resp: &Response, rect: Rect, selected: bool, force: bool) {
    let time = if look().motion { 0.12 } else { 0.0 };
    let h = ui.ctx().animate_bool_with_time(resp.id.with("bg"), resp.hovered() || force, time);
    let p = c();
    if selected {
        ui.painter().rect_filled(rect, 8.0, p.selected);
    } else if h > 0.0 {
        ui.painter().rect_filled(rect, 8.0, p.hover.gamma_multiply(h));
    }
}

pub enum RowClick {
    Play,
    PlayNext,
    Enqueue,
    Radio,
    Album,
    Artist(usize),
    Like,
    AddTo(usize),
    Remove,
    MoveUp,
    MoveDown,
    /// The row started being dragged (reorderable lists only).
    DragStart,
}

/// What the track context menu can offer.
pub struct RowCtx<'a> {
    pub liked: bool,
    pub playlists: &'a [MusicPlaylistItem],
    pub removable: bool,
    pub compact: bool,
    pub show_album: bool,
    /// Offers move up/down and can be dragged.
    pub movable: bool,
}

pub fn track_row(ui: &mut Ui, t: &TrackItem, playing: bool, rc: &RowCtx) -> Option<RowClick> {
    track_row_at(ui, t, playing, rc).0
}

/// Like `track_row`, also returning where the row was laid out (for drag-and-drop targets).
pub fn track_row_at(ui: &mut Ui, t: &TrackItem, playing: bool, rc: &RowCtx) -> (Option<RowClick>, Rect) {
    let p = c();
    let row_h = if rc.compact { ROW_H_COMPACT } else { ROW_H };
    let w = ui.available_width();
    let sense = if rc.movable { Sense::click_and_drag() } else { Sense::click() };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, row_h), sense);
    if !ui.is_rect_visible(rect) {
        return (None, rect);
    }
    let menu_open = resp.context_menu_opened();
    hover_bg(ui, &resp, rect, false, menu_open);
    let hovered = resp.hovered() || menu_open;
    let cy = rect.center().y;
    let painter = ui.painter();

    let x = if rc.compact {
        let c = Rect::from_center_size(pos2(rect.left() + 22.0, cy), vec2(20.0, 20.0));
        if playing {
            eq_bars(painter, c.center(), p.accent);
        } else if hovered {
            paint_icon(painter, Icon::Play, c.shrink(3.0), p.text);
        }
        rect.left() + 44.0
    } else {
        let c = Rect::from_min_size(pos2(rect.left() + 8.0, cy - 20.0), vec2(40.0, 40.0));
        paint_cover(ui, &t.cover, c, 2);
        if playing || hovered {
            painter.rect_filled(c, 2.0, Color32::from_black_alpha(150));
            if playing { eq_bars(painter, c.center(), Color32::WHITE) } else { paint_icon(painter, Icon::Play, c.shrink(10.0), Color32::WHITE) }
        }
        c.right() + 16.0
    };

    let avail = rect.right() - 96.0 - x;
    let album = t.album.as_ref().filter(|_| rc.show_album && avail > 520.0);
    let title_w = if album.is_some() { avail * 0.55 } else { avail };
    let title_col = if playing { p.accent } else { p.text };
    // Artist line: hovering an artist with a page underlines the line and makes it a link.
    let by = artists(t);
    let link_artist = t.artists.iter().position(|a| a.id.is_some());
    let (title_pos, by_pos) = if rc.compact {
        (pos2(x, cy - 9.0), None)
    } else {
        (pos2(x, cy - 19.0), Some(pos2(x, cy + 1.0)))
    };
    let title_w = if rc.compact { title_w * 0.6 } else { title_w };
    painter.galley(title_pos, line(ui, &t.name, medium(14.0), title_col, title_w), title_col);
    let by_galley = line(ui, &by, regular(13.0), p.text2, if rc.compact { title_w * 0.6 } else { title_w });
    let by_pos = by_pos.unwrap_or(pos2(x + title_w + 16.0, cy - 8.0));
    let by_rect = Rect::from_min_size(by_pos, by_galley.size());
    let on_artist = link_artist.is_some() && resp.hover_pos().is_some_and(|h| by_rect.contains(h));
    painter.galley(by_pos, by_galley, p.text2);
    if on_artist {
        painter.line_segment([by_rect.left_bottom(), by_rect.right_bottom()], Stroke::new(1.0, p.text2));
    }
    if let Some(a) = album {
        let g = line(ui, &a.name, regular(13.0), p.text2, avail * 0.4);
        painter.galley(pos2(x + avail * 0.6, cy - g.size().y / 2.0), g, p.text2);
    }
    if rc.liked {
        let hr = Rect::from_center_size(pos2(rect.right() - 76.0, cy), vec2(16.0, 16.0));
        paint_icon(painter, Icon::Heart, hr, p.text2);
    }
    if let Some(d) = t.duration {
        let g = line(ui, &fmt_time(d.into()), regular(13.0), p.text2, 60.0);
        painter.galley(pos2(rect.right() - 16.0 - g.size().x, cy - g.size().y / 2.0), g, p.text2);
    }

    let mut out = None;
    if rc.movable && resp.drag_started() {
        out = Some(RowClick::DragStart);
    }
    if resp.clicked() {
        out = Some(match link_artist.filter(|_| on_artist) {
            Some(i) => RowClick::Artist(i),
            None => RowClick::Play,
        });
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).context_menu(|ui| {
        ui.set_min_width(210.0);
        let item = |ui: &mut Ui, label: &str| ui.button(label).clicked();
        if item(ui, tr("Tocar")) {
            out = Some(RowClick::Play);
        }
        if item(ui, tr("Tocar a seguir")) {
            out = Some(RowClick::PlayNext);
        }
        if item(ui, tr("Adicionar à fila")) {
            out = Some(RowClick::Enqueue);
        }
        if item(ui, tr("Iniciar rádio")) {
            out = Some(RowClick::Radio);
        }
        if item(ui, tr("Copiar link")) {
            ui.ctx().copy_text(crate::yt_url(t));
        }
        ui.separator();
        if item(ui, if rc.liked { tr("Remover das curtidas") } else { tr("Curtir") }) {
            out = Some(RowClick::Like);
        }
        if !rc.playlists.is_empty() {
            ui.menu_button(tr("Adicionar à playlist"), |ui| {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (i, pl) in rc.playlists.iter().enumerate() {
                        if ui.button(&pl.name).clicked() {
                            out = Some(RowClick::AddTo(i));
                        }
                    }
                });
            });
        }
        if rc.movable && item(ui, tr("Mover para cima")) {
            out = Some(RowClick::MoveUp);
        }
        if rc.movable && item(ui, tr("Mover para baixo")) {
            out = Some(RowClick::MoveDown);
        }
        if rc.removable && item(ui, tr("Remover daqui")) {
            out = Some(RowClick::Remove);
        }
        ui.separator();
        for (i, a) in t.artists.iter().enumerate().filter(|(_, a)| a.id.is_some()) {
            if item(ui, &tf!("Ir para {}", a.name)) {
                out = Some(RowClick::Artist(i));
            }
        }
        if t.album.is_some() && item(ui, tr("Ir para o álbum")) {
            out = Some(RowClick::Album);
        }
    });
    (out, rect)
}

/// Square (or round, for artists) cover with title and subtitle underneath, as on YTM shelves.
/// `stack` draws two sleeves behind the cover so playlists read differently from albums.
pub fn card(ui: &mut Ui, t: &[Thumbnail], title: &str, sub: &str, size: f32, stack: bool, round: bool) -> Response {
    let p = c();
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size + 58.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let cr = Rect::from_min_size(rect.min + vec2(0.0, 10.0), vec2(size, size));
    if stack {
        let painter = ui.painter();
        painter.rect_filled(cr.shrink2(vec2(14.0, 0.0)).translate(vec2(0.0, -10.0)), 4.0, p.selected.gamma_multiply(0.7));
        painter.rect_filled(cr.shrink2(vec2(7.0, 0.0)).translate(vec2(0.0, -5.0)), 4.0, p.selected.lerp_to_gamma(p.text2, 0.25));
    }
    let radius = if round { (size / 2.0) as u8 } else { 6 };
    // A soft panel behind the whole card (art and text) while hovered.
    let lift = ui.ctx().animate_bool_with_time(resp.id.with("panel"), resp.hovered(), if look().motion { 0.15 } else { 0.0 });
    if lift > 0.01 {
        ui.painter().rect_filled(rect.expand2(vec2(8.0, 2.0)), 10.0, p.wash().gamma_multiply(lift));
    }
    let hov = tilt_cover(ui, &resp, cr, t, radius, 0.28);
    let painter = ui.painter();
    if hov > 0.01 && !round {
        let b = Rect::from_center_size(cr.right_bottom() - vec2(28.0, 28.0), vec2(36.0, 36.0));
        painter.circle_filled(b.center(), 18.0, Color32::from_black_alpha((hov * 200.0) as u8));
        paint_icon(painter, Icon::Play, b.shrink(8.0), Color32::from_white_alpha((hov * 255.0) as u8));
    }
    let align = |g: &Arc<Galley>| if round { cr.center().x - g.size().x / 2.0 } else { cr.left() };
    let tg = line(ui, title, medium(14.0), p.text, size);
    let sg = line(ui, sub, regular(13.0), p.text2, size);
    painter.galley(pos2(align(&tg), cr.bottom() + 8.0), tg, p.text);
    painter.galley(pos2(align(&sg), cr.bottom() + 28.0), sg, p.text2);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(24.0);
    ui.label(egui::RichText::new(title).font(bold(22.0)).color(c().text));
}

/// Horizontal row of cards with a section title. Returns the clicked index.
pub fn shelf<T>(ui: &mut Ui, title: &str, items: &[T], stack: bool, round: bool, f: impl Fn(&T) -> (&[Thumbnail], &str, String)) -> Option<usize> {
    shelf_by(ui, title, items, stack, |_| round, f)
}

/// `shelf` where each card decides if it's round (artists among albums and playlists).
pub fn shelf_by<T>(ui: &mut Ui, title: &str, items: &[T], stack: bool, round: impl Fn(&T) -> bool, f: impl Fn(&T) -> (&[Thumbnail], &str, String)) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    let mut clicked = None;
    section(ui, title);
    ui.add_space(4.0);
    egui::ScrollArea::horizontal().id_salt(title).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            ui.add_space(6.0);
            for (i, it) in items.iter().enumerate() {
                let (img, name, sub) = f(it);
                if card(ui, img, name, &sub, 168.0, stack, round(it)).clicked() {
                    clicked = Some(i);
                }
            }
            ui.add_space(6.0);
        });
    });
    clicked
}

/// Full-row nav entry for the sidebar.
pub fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let p = c();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    hover_bg(ui, &resp, rect, selected, false);
    let painter = ui.painter();
    paint_icon(painter, icon, Rect::from_min_size(pos2(rect.left() + 14.0, rect.center().y - 11.0), vec2(22.0, 22.0)), p.text);
    let g = line(ui, label, medium(14.0), p.text, rect.width() - 64.0);
    painter.galley(pos2(rect.left() + 52.0, rect.center().y - g.size().y / 2.0), g, p.text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Artwork for library rows and quick tiles.
#[derive(Clone, Copy)]
pub enum Art<'a> {
    Cover(&'a [Thumbnail]),
    /// "Liked songs": accent gradient with a heart, like YTM's auto playlist tile.
    Liked,
}

fn paint_art(ui: &Ui, art: Art, rect: Rect, radius: u8) {
    match art {
        Art::Cover(t) => paint_cover(ui, t, rect, radius),
        Art::Liked => {
            let p = c();
            let mut mesh = egui::Mesh::default();
            let (a, b) = (p.accent, p.accent.lerp_to_gamma(Color32::from_rgb(110, 60, 255), 0.75));
            mesh.colored_vertex(rect.left_top(), a);
            mesh.colored_vertex(rect.right_top(), a.lerp_to_gamma(b, 0.5));
            mesh.colored_vertex(rect.left_bottom(), a.lerp_to_gamma(b, 0.5));
            mesh.colored_vertex(rect.right_bottom(), b);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(1, 2, 3);
            ui.painter().add(Shape::mesh(mesh));
            paint_icon(ui.painter(), Icon::Heart, rect.shrink(rect.width() * 0.28), Color32::WHITE);
        }
    }
}

/// Library row with artwork; `compact` = name only.
pub fn lib_item(ui: &mut Ui, art: Art, title: &str, sub: &str, selected: bool, compact: bool) -> Response {
    let p = c();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), if compact { 34.0 } else { 56.0 }), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    hover_bg(ui, &resp, rect, selected, false);
    let painter = ui.painter();
    if compact {
        let g = line(ui, title, regular(14.0), p.text, rect.width() - 28.0);
        painter.galley(pos2(rect.left() + 14.0, rect.center().y - g.size().y / 2.0), g, p.text);
    } else {
        let a = Rect::from_min_size(pos2(rect.left() + 8.0, rect.center().y - 20.0), vec2(40.0, 40.0));
        paint_art(ui, art, a, 4);
        let x = a.right() + 12.0;
        let w = rect.right() - x - 8.0;
        let painter = ui.painter();
        painter.galley(pos2(x, rect.center().y - 18.0), line(ui, title, medium(14.0), p.text, w), p.text);
        painter.galley(pos2(x, rect.center().y + 2.0), line(ui, sub, regular(12.0), p.text2, w), p.text2);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Home quick-access tile: artwork + name on a raised surface; a play button slides in on hover.
/// Returns (opened, play pressed).
pub fn quick_tile(ui: &mut Ui, art: Art, title: &str, width: f32, tint: Option<Color32>) -> (bool, bool) {
    let p = c();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 64.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return (false, false);
    }
    let h = ui.ctx().animate_bool_with_time(resp.id.with("qt"), resp.hovered(), if look().motion { 0.15 } else { 0.0 });
    ui.painter().rect_filled(rect, 6.0, p.bar.lerp_to_gamma(p.selected, h));
    // The cover's colour washing out of the artwork towards the title, stronger on hover.
    if let Some(t) = tint {
        let strength = if p.dark { 0.30 + 0.16 * h } else { 0.16 + 0.10 * h };
        let wash = Rect::from_min_max(rect.min + vec2(40.0, 0.0), rect.max);
        let mut mesh = egui::Mesh::default();
        let (from, to) = (t.gamma_multiply(strength), Color32::TRANSPARENT);
        for (pos, col) in [(wash.left_top(), from), (wash.right_top(), to), (wash.right_bottom(), to), (wash.left_bottom(), from)] {
            mesh.colored_vertex(pos, col);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ui.painter().with_clip_rect(rect.shrink2(vec2(0.0, 0.5))).add(Shape::mesh(mesh));
    }
    let a = Rect::from_min_size(rect.min, vec2(64.0, 64.0));
    paint_art(ui, art, a, 6);
    let painter = ui.painter();
    let play = Rect::from_center_size(pos2(rect.right() - 30.0, rect.center().y), vec2(40.0, 40.0));
    let text_w = rect.width() - 64.0 - 24.0 - if h > 0.01 { 48.0 } else { 0.0 };
    let g = line(ui, title, bold(15.0), p.text, text_w);
    painter.galley(pos2(a.right() + 14.0, rect.center().y - g.size().y / 2.0), g, p.text);
    let mut pressed = false;
    if h > 0.01 {
        painter.add(egui::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha((90.0 * h) as u8) }.as_shape(play, 20));
        painter.circle_filled(play.center(), 20.0, p.accent.gamma_multiply(h));
        paint_icon(painter, Icon::Play, play.shrink(11.0), Color32::WHITE.gamma_multiply(h));
        pressed = resp.clicked() && resp.interact_pointer_pos().is_some_and(|q| play.contains(q));
    }
    let opened = resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !pressed;
    (opened, pressed)
}

/// Small filter chip (YTM's "Playlists / Álbuns" row).
pub fn chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let p = c();
    let fg = if selected { p.bg } else { p.text };
    let g = ui.painter().layout_no_wrap(label.into(), medium(13.0), fg);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 24.0, 30.0), Sense::click());
    let fill = if selected { p.text } else if resp.hovered() { p.selected } else { p.wash() };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The main play / pause control: a filled disc, the one thing in the bar that should pop.
pub fn play_button(ui: &mut Ui, paused: bool, size: f32) -> Response {
    let p = c();
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let grow = ui.ctx().animate_bool_with_time(resp.id.with("pb"), resp.hovered(), if look().motion { 0.1 } else { 0.0 });
    let r = size / 2.0 * (1.0 + 0.06 * grow);
    ui.painter().circle_filled(rect.center(), r, p.text);
    let icon = if paused { Icon::Play } else { Icon::Pause };
    paint_icon(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(size, size) * 0.52), p.bg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Fade + slide-up for content that just appeared. `since` = seconds since it appeared.
pub fn appear(ui: &mut Ui, since: f32) {
    if !look().motion {
        return;
    }
    let p = ease(since / 0.25);
    ui.multiply_opacity(p);
    ui.add_space((1.0 - p) * 16.0);
}

/// Opacity for the i-th visible row of a list that appeared `since` seconds ago (cascade).
pub fn cascade(since: f32, i: usize) -> f32 {
    if !look().motion {
        return 1.0;
    }
    ease((since - i as f32 * 0.025) / 0.2)
}

/// Thin YTM-style bar. Returns the response and the fraction under the pointer while pressed.
pub fn seek_bar(ui: &mut Ui, rect: Rect, id: &str, frac: f32, fill: Color32) -> (Response, f32) {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click_and_drag());
    let f = resp.interact_pointer_pos().map_or(frac, |p| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
    let hot = resp.hovered() || resp.dragged();
    let h = if hot { 4.0 } else { 2.0 };
    let bar = Rect::from_center_size(rect.center(), vec2(rect.width(), h));
    let painter = ui.painter();
    painter.rect_filled(bar, 1.0, c().text.gamma_multiply(0.18));
    let x = bar.left() + bar.width() * f;
    painter.rect_filled(Rect::from_min_max(bar.min, pos2(x, bar.bottom())), 1.0, fill);
    if hot {
        painter.circle_filled(pos2(x, bar.center().y), 6.0, fill);
    }
    (resp.on_hover_cursor(egui::CursorIcon::PointingHand), f)
}

/// Dominant, saturated colour of an encoded image (for tinting pages with album art).
pub fn art_color(bytes: &[u8]) -> Option<Color32> {
    let img = image::load_from_memory(bytes).ok()?.thumbnail(24, 24).to_rgb8();
    // Weight pixels by saturation so grey backgrounds don't wash the colour out.
    let (mut acc, mut wsum) = ([0f32; 3], 0f32);
    for p in img.pixels() {
        let [r, g, b] = p.0.map(|v| v as f32);
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        let w = (mx - mn) / 255.0 + 0.05;
        acc = [acc[0] + r * w, acc[1] + g * w, acc[2] + b * w];
        wsum += w;
    }
    let [r, g, b] = acc.map(|v| v / wsum);
    // Keep it usable as a background tint: not too dark, not blown out.
    let mx = r.max(g).max(b).max(1.0);
    let k = (200.0 / mx).min(1.6);
    Some(Color32::from_rgb((r * k).min(255.0) as u8, (g * k).min(255.0) as u8, (b * k).min(255.0) as u8))
}


/// Moves the window by dragging `resp`, but only once the pointer has really moved: starting the
/// compositor's move on press would swallow clicks on buttons drawn over the drag area.
pub fn drag_window(ui: &Ui, resp: &Response) {
    let sent = resp.id.with("win-drag");
    if !resp.dragged() {
        ui.ctx().data_mut(|d| d.remove::<bool>(sent));
        return;
    }
    let already = ui.ctx().data(|d| d.get_temp::<bool>(sent).unwrap_or(false));
    if !already && ui.input(|i| i.pointer.is_decidedly_dragging()) {
        ui.ctx().data_mut(|d| d.insert_temp(sent, true));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_and_time() {
        let (url, uv) = img_src("https://lh3.googleusercontent.com/abc=w60-h60-l90-rj", 60, 60, 400);
        assert_eq!(url, "https://lh3.googleusercontent.com/abc=w400-h400-p-l90-rj");
        // Other Google hosts and the `=s` form get resized too.
        assert_eq!(img_src("https://yt3.ggpht.com/abc=s192", 192, 192, 300).0, "https://yt3.ggpht.com/abc=w300-h300-p-l90-rj");
        assert_eq!(uv, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
        // A wide channel photo: the server returns it square, so no second crop.
        let (_, uv) = img_src("https://yt3.googleusercontent.com/x=w540-h225-p-l90-rj", 540, 225, 168);
        assert_eq!(uv, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)));
        let (url, uv) = img_src("https://i.ytimg.com/vi/x/hq.jpg", 480, 270, 40);
        assert_eq!(url, "https://i.ytimg.com/vi/x/hq.jpg");
        assert!((uv.width() - 0.5625).abs() < 1e-6 && (uv.center().x - 0.5).abs() < 1e-6);
        // 4:3 video thumbnails drop their black bars: a centred 270 x 270 square of 480 x 360.
        let (_, uv) = img_src("https://i.ytimg.com/vi/x/hqdefault.jpg", 480, 360, 100);
        assert!((uv.width() - 0.5625).abs() < 1e-6 && (uv.min.y - 0.125).abs() < 1e-6 && (uv.max.y - 0.875).abs() < 1e-6);
        assert!(img(&[], 40).is_none());
        assert_eq!(fmt_time(65.9), "1:05");
        assert_eq!(fmt_time(3725.0), "1:02:05");
    }

    #[test]
    fn art_color_picks_the_saturated_part() {
        let mut im = image::RgbImage::from_pixel(8, 8, image::Rgb([128, 128, 128]));
        for x in 0..4 {
            for y in 0..8 {
                im.put_pixel(x, y, image::Rgb([200, 20, 20]));
            }
        }
        let mut png = vec![];
        im.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let col = art_color(&png).unwrap();
        assert!(col.r() > col.g() + 40 && col.r() > col.b() + 40, "{col:?}");
    }
}

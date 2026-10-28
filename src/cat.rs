//! Draws Mew: the pill, the cat, sound bars, thinking dots and speech bubbles. Pure drawing, no OS code.

use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use tiny_skia::{FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// Logical canvas (scaled by DPI). The pill sits at the bottom; bubbles float above it.
pub const CANVAS_W: f32 = 420.0;
pub const CANVAS_H: f32 = 160.0;
pub const PILL_H_IDLE: f32 = 30.0;
const PILL_H_ACTIVE: f32 = 38.0;

type Rgba = (u8, u8, u8, u8);
const BG: Rgba = (21, 21, 27, 238);
const BORDER: Rgba = (255, 255, 255, 46);
const ACCENT: Rgba = (255, 64, 105, 255); // loud pink-red while listening
const FUR: Rgba = (255, 176, 76, 255);
const FUR_DARK: Rgba = (214, 128, 40, 255);
const EAR_IN: Rgba = (255, 143, 163, 255);
const INK: Rgba = (24, 22, 28, 255);
const WHITE: Rgba = (255, 255, 255, 255);
const WHISKER: Rgba = (255, 236, 214, 200);
const BUBBLE: Rgba = (255, 255, 255, 250);
const BUBBLE_TEXT: Rgba = (24, 22, 28, 255);
const TRACK: Rgba = (226, 222, 230, 255);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Idle,
    Listening,
    Thinking,
    Said,
    Huh,
}

impl Mode {
    pub fn pill_width(self) -> f32 {
        match self {
            Mode::Idle | Mode::Said | Mode::Huh => 64.0,
            Mode::Listening => 176.0,
            Mode::Thinking => 112.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub mode: Mode,
    pub t: f32,           // seconds since the mode started
    pub pill_w: f32,      // animated width
    pub levels: Vec<f32>, // recent mic levels 0..1, newest last
    pub locked: bool,     // hands-free recording
    pub text: String,     // bubble text; empty = no bubble
    pub progress: Option<f32>,
}

pub struct Renderer {
    font: Option<FontArc>,
}

impl Renderer {
    pub fn new() -> Self {
        let font =
            font_paths().iter().find_map(|path| std::fs::read(path).ok().and_then(|b| FontArc::try_from_vec(b).ok()));
        Self { font }
    }

    pub fn render(&self, scene: &Scene, scale: f32) -> Pixmap {
        let mut c = Canvas::new((CANVAS_W * scale).round() as u32, (CANVAS_H * scale).round() as u32, scale);
        let active = scene.mode != Mode::Idle;
        let ph = if active { PILL_H_ACTIVE } else { PILL_H_IDLE };
        let pw = scene.pill_w.max(ph);
        let cx = CANVAS_W / 2.0;
        let bottom = CANVAS_H - 4.0;
        let (x0, y0, x1) = (cx - pw / 2.0, bottom - ph, cx + pw / 2.0);

        let pill = rounded_rect(x0, y0, x1, bottom, ph / 2.0);
        c.fill(&pill, BG);
        c.stroke(&pill, if scene.mode == Mode::Listening { ACCENT } else { BORDER }, 1.5);

        // The cat sits at the left of a wide pill (centered in a narrow one), ears poking over the top edge.
        let head = if active { 22.0 } else { 17.0 };
        let cat_x = if pw > Mode::Idle.pill_width() + 8.0 { x0 + ph / 2.0 + 4.0 } else { cx };
        draw_cat(&mut c, cat_x, y0 + ph / 2.0 + 2.0, head, scene.mode, scene.t);

        match scene.mode {
            Mode::Listening => {
                let right = if scene.locked { x1 - ph / 2.0 - 14.0 } else { x1 - ph / 2.0 };
                draw_bars(&mut c, cat_x + head * 0.9 + 6.0, right, y0 + ph / 2.0, ph * 0.62, &scene.levels);
                if scene.locked {
                    draw_lock(&mut c, x1 - ph / 2.0 - 4.0, y0 + ph / 2.0);
                }
            }
            Mode::Thinking => draw_dots(&mut c, cat_x + head * 0.9 + 10.0, y0 + ph / 2.0, scene.t),
            Mode::Idle => self.text(&mut c, cat_x + head * 0.75, y0 - 4.0, "z", 9.0, (255, 255, 255, 150)),
            _ => {}
        }
        if !scene.text.is_empty() {
            self.bubble(&mut c, cat_x, y0 - 6.0, &scene.text, scene.progress);
        }
        c.pixmap
    }

    fn bubble(&self, c: &mut Canvas, anchor_x: f32, bottom: f32, text: &str, progress: Option<f32>) {
        const MAX_W: f32 = 340.0;
        const PAD: f32 = 10.0;
        const LINE_H: f32 = 17.0;
        const SIZE: f32 = 13.0;
        let lines = self.wrap(text, SIZE, MAX_W, 3);
        let text_w = lines.iter().map(|l| self.measure(l, SIZE)).fold(0.0, f32::max);
        let bar_h = if progress.is_some() { 12.0 } else { 0.0 };
        let bw = (text_w + PAD * 2.0).max(if progress.is_some() { 220.0 } else { 0.0 });
        let bh = lines.len() as f32 * LINE_H + PAD * 2.0 - 3.0 + bar_h;
        let left = (anchor_x - 24.0).max(4.0).min(CANVAS_W - 4.0 - bw);
        let top = bottom - 8.0 - bh;
        c.fill(&rounded_rect(left, top, left + bw, top + bh, 10.0), BUBBLE);
        let tail_x = anchor_x.clamp(left + 14.0, left + bw - 14.0);
        c.fill(&polygon(&[(tail_x - 6.0, top + bh - 1.0), (tail_x + 6.0, top + bh - 1.0), (tail_x, bottom)]), BUBBLE);
        for (i, line) in lines.iter().enumerate() {
            self.text(c, left + PAD, top + PAD - 2.0 + i as f32 * LINE_H, line, SIZE, BUBBLE_TEXT);
        }
        if let Some(p) = progress {
            let (bx0, bx1, by) = (left + PAD, left + bw - PAD, top + bh - PAD - 5.0);
            c.fill(&rounded_rect(bx0, by, bx1, by + 5.0, 2.5), TRACK);
            let fill_to = bx0 + (bx1 - bx0) * p.clamp(0.0, 1.0);
            if fill_to > bx0 + 5.0 {
                c.fill(&rounded_rect(bx0, by, fill_to, by + 5.0, 2.5), ACCENT);
            }
        }
    }

    fn measure(&self, s: &str, size: f32) -> f32 {
        let Some(font) = &self.font else { return s.len() as f32 * size * 0.5 };
        let f = font.as_scaled(PxScale::from(size));
        let mut prev = None;
        s.chars()
            .map(|ch| {
                let id = f.glyph_id(ch);
                let kern = prev.map_or(0.0, |p| f.kern(p, id));
                prev = Some(id);
                kern + f.h_advance(id)
            })
            .sum()
    }

    fn wrap(&self, text: &str, size: f32, max_w: f32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut truncated = false;
        for word in text.split_whitespace() {
            let trial = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
            if self.measure(&trial, size) <= max_w {
                current = trial;
                continue;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if lines.len() == max_lines {
                truncated = true;
                break;
            }
            current = word.to_string();
        }
        if !current.is_empty() {
            if lines.len() < max_lines {
                lines.push(current);
            } else {
                truncated = true;
            }
        }
        if truncated && let Some(last) = lines.last_mut() {
            while !last.is_empty() && self.measure(&format!("{last}…"), size) > max_w {
                last.pop();
            }
            *last = format!("{}…", last.trim_end());
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }

    /// Draws `s` with its top-left at (x, y) in logical units.
    fn text(&self, c: &mut Canvas, x: f32, y: f32, s: &str, size: f32, color: Rgba) {
        let Some(font) = &self.font else { return };
        let px = size * c.scale;
        let f = font.as_scaled(PxScale::from(px));
        let (mut pen, baseline) = (x * c.scale, y * c.scale + f.ascent());
        let mut prev = None;
        for ch in s.chars() {
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                pen += f.kern(p, id);
            }
            prev = Some(id);
            let glyph = id.with_scale_and_position(px, point(pen, baseline));
            pen += f.h_advance(id);
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    c.blend_pixel(bounds.min.x as i32 + gx as i32, bounds.min.y as i32 + gy as i32, color, coverage);
                });
            }
        }
    }
}

/// System fonts for the bubble text, best first.
#[cfg(windows)]
fn font_paths() -> Vec<String> {
    let fonts = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()) + r"\Fonts\";
    ["segoeui.ttf", "arial.ttf"].iter().map(|name| format!("{fonts}{name}")).collect()
}

#[cfg(target_os = "macos")]
fn font_paths() -> Vec<String> {
    ["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc"].map(String::from).into()
}

fn icon_pixmap(size: u32) -> Pixmap {
    let mut c = Canvas::new(size, size, size as f32 / 32.0);
    draw_cat(&mut c, 16.0, 19.0, 21.0, Mode::Listening, 0.0);
    c.pixmap
}

/// Square RGBA (straight alpha) cat face for the tray icon.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    icon_pixmap(size)
        .pixels()
        .iter()
        .flat_map(|p| {
            let p = p.demultiply();
            [p.red(), p.green(), p.blue(), p.alpha()]
        })
        .collect()
}

pub fn icon_png(size: u32) -> Vec<u8> {
    icon_pixmap(size).encode_png().unwrap_or_default()
}

/// Multi-size .ico (PNG entries) for the executable and installer.
pub fn icon_ico() -> Vec<u8> {
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let pngs: Vec<Vec<u8>> = sizes.iter().map(|&s| icon_png(s)).collect();
    let mut ico = vec![0, 0, 1, 0];
    ico.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&size, png) in sizes.iter().zip(&pngs) {
        let dim = if size >= 256 { 0 } else { size as u8 };
        ico.extend([dim, dim, 0, 0, 1, 0, 32, 0]);
        ico.extend((png.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in pngs {
        ico.extend(png);
    }
    ico
}

/// .icns (PNG entries, 16 to 1024 px) for the macOS app bundle.
pub fn icon_icns() -> Vec<u8> {
    let entries = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"ic11", 32),
        (b"icp6", 64),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic13", 256),
        (b"ic08", 256),
        (b"ic14", 512),
        (b"ic09", 512),
        (b"ic10", 1024),
    ];
    let mut body = Vec::new();
    for (kind, size) in entries {
        let png = icon_png(size);
        body.extend(kind);
        body.extend((png.len() as u32 + 8).to_be_bytes());
        body.extend(png);
    }
    let mut icns = b"icns".to_vec();
    icns.extend((body.len() as u32 + 8).to_be_bytes());
    icns.extend(body);
    icns
}

struct Canvas {
    pixmap: Pixmap,
    scale: f32,
}

impl Canvas {
    fn new(w: u32, h: u32, scale: f32) -> Self {
        Self { pixmap: Pixmap::new(w.max(1), h.max(1)).expect("canvas size"), scale }
    }

    fn paint(color: Rgba) -> Paint<'static> {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.0, color.1, color.2, color.3);
        paint.anti_alias = true;
        paint
    }

    fn fill(&mut self, path: &Option<Path>, color: Rgba) {
        if let Some(path) = path {
            let t = Transform::from_scale(self.scale, self.scale);
            self.pixmap.fill_path(path, &Self::paint(color), FillRule::Winding, t, None);
        }
    }

    fn stroke(&mut self, path: &Option<Path>, color: Rgba, width: f32) {
        if let Some(path) = path {
            let stroke = Stroke { width, line_cap: LineCap::Round, ..Stroke::default() };
            let t = Transform::from_scale(self.scale, self.scale);
            self.pixmap.stroke_path(path, &Self::paint(color), &stroke, t, None);
        }
    }

    /// Source-over blend of one pixel at physical coordinates, for glyph coverage.
    fn blend_pixel(&mut self, x: i32, y: i32, color: Rgba, coverage: f32) {
        let (w, h) = (self.pixmap.width() as i32, self.pixmap.height() as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let a = color.3 as f32 / 255.0 * coverage.clamp(0.0, 1.0);
        let i = ((y * w + x) * 4) as usize;
        let data = self.pixmap.data_mut();
        for (k, ch) in [color.0, color.1, color.2].into_iter().enumerate() {
            data[i + k] = (ch as f32 * a + data[i + k] as f32 * (1.0 - a)).round() as u8;
        }
        data[i + 3] = (255.0 * a + data[i + 3] as f32 * (1.0 - a)).round() as u8;
    }
}

fn draw_cat(c: &mut Canvas, cx: f32, cy: f32, size: f32, mode: Mode, t: f32) {
    let (w, h) = (size * 1.18, size);
    let perk = match mode {
        Mode::Listening | Mode::Said => 1.0,
        Mode::Thinking | Mode::Huh => 0.8,
        Mode::Idle => 0.62,
    };

    // Ears: taller and more upright when perked.
    for side in [-1.0f32, 1.0] {
        let bx = cx + side * w * 0.30;
        let tip = (cx + side * w * (0.40 + (1.0 - perk) * 0.25), cy - h * (0.42 + 0.50 * perk));
        c.fill(&polygon(&[(bx - w * 0.20, cy - h * 0.22), tip, (bx + w * 0.20, cy - h * 0.22)]), FUR);
        let inner_tip = (tip.0 - side * w * 0.02, tip.1 + h * 0.16);
        c.fill(&polygon(&[(bx - w * 0.10, cy - h * 0.26), inner_tip, (bx + w * 0.10, cy - h * 0.26)]), EAR_IN);
    }
    c.fill(&oval(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0), FUR);
    for dx in [-0.12, 0.0, 0.12] {
        let x = cx + dx * w;
        c.stroke(&line(x, cy - h * 0.46, x, cy - h * 0.30), FUR_DARK, 1.4);
    }

    let (ey, ex, er) = (cy - h * 0.02, w * 0.22, size * 0.13);
    let blink = mode == Mode::Listening && t % 3.2 > 3.05;
    for side in [-1.0f32, 1.0] {
        let x = cx + side * ex;
        if mode == Mode::Idle || blink {
            c.stroke(&arc(x - er, ey - er * 1.3, x + er, ey + er * 0.5, 20.0, 160.0), INK, 1.6); // ‿ asleep
        } else if mode == Mode::Said {
            c.stroke(&arc(x - er, ey - er * 0.2, x + er, ey + er * 1.6, 200.0, 340.0), INK, 1.8); // ^ happy
        } else {
            c.fill(&oval(x - er, ey - er * 1.1, x + er, ey + er * 1.1), INK);
            let (gx, gy) = match mode {
                Mode::Thinking => (0.35, -0.55),
                Mode::Huh => (-0.3, -0.3),
                _ => (0.25, -0.4),
            };
            let g = er * 0.38;
            let (px, py) = (x + gx * er, ey + gy * er);
            c.fill(&oval(px - g, py - g, px + g, py + g), WHITE);
        }
    }

    let (ny, n, m) = (cy + h * 0.16, size * 0.06, size * 0.09);
    c.fill(&polygon(&[(cx - n, ny - n * 0.6), (cx + n, ny - n * 0.6), (cx, ny + n * 0.5)]), EAR_IN);
    if matches!(mode, Mode::Said | Mode::Huh) {
        c.fill(&oval(cx - m * 0.7, ny + n * 0.6, cx + m * 0.7, ny + n * 0.6 + m * 1.3), INK);
    } else {
        for left in [cx - m, cx] {
            c.stroke(&arc(left, ny, left + m, ny + m, 0.0, 180.0), INK, 1.2); // ω
        }
    }

    for side in [-1.0f32, 1.0] {
        for (i, dy) in [-0.04f32, 0.08].into_iter().enumerate() {
            let (x_in, x_out, y) = (cx + side * w * 0.36, cx + side * w * 0.66, cy + h * (0.14 + dy));
            c.stroke(&line(x_in, y, x_out, y + (i as f32 - 0.5) * h * 0.12), WHISKER, 0.9);
        }
    }
}

fn draw_bars(c: &mut Canvas, x0: f32, x1: f32, cy: f32, max_h: f32, levels: &[f32]) {
    const N: usize = 18;
    let gap = (x1 - x0) / N as f32;
    let bw = gap * 0.5;
    let recent = &levels[levels.len().saturating_sub(N)..];
    for i in 0..N {
        let level = (i + recent.len()).checked_sub(N).map_or(0.0, |j| recent[j]);
        let bh = (max_h * level.min(1.0)).max(bw);
        let x = x0 + i as f32 * gap + gap / 2.0;
        c.fill(&rounded_rect(x - bw / 2.0, cy - bh / 2.0, x + bw / 2.0, cy + bh / 2.0, bw / 2.0), ACCENT);
    }
}

fn draw_dots(c: &mut Canvas, x0: f32, cy: f32, t: f32) {
    for i in 0..3 {
        let bounce = (t * 7.0 - i as f32 * 0.9).sin().max(0.0) * 4.0;
        let (x, r) = (x0 + i as f32 * 13.0, 3.2);
        c.fill(&oval(x - r, cy - r - bounce, x + r, cy + r - bounce), (255, 255, 255, 220));
    }
}

fn draw_lock(c: &mut Canvas, cx: f32, cy: f32) {
    c.stroke(&arc(cx - 3.0, cy - 7.0, cx + 3.0, cy - 1.0, 180.0, 360.0), WHITE, 1.6);
    c.stroke(&line(cx - 3.0, cy - 4.0, cx - 3.0, cy - 1.0), WHITE, 1.6);
    c.stroke(&line(cx + 3.0, cy - 4.0, cx + 3.0, cy - 1.0), WHITE, 1.6);
    c.fill(&rounded_rect(cx - 4.5, cy - 1.5, cx + 4.5, cy + 5.0, 1.5), WHITE);
}

fn rounded_rect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Option<Path> {
    let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
    let k = r * 0.552_284_8; // cubic Bézier circle constant
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + r, y0);
    pb.line_to(x1 - r, y0);
    pb.cubic_to(x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
    pb.line_to(x0 + r, y1);
    pb.cubic_to(x0 + r - k, y1, x0, y1 - r + k, x0, y1 - r);
    pb.line_to(x0, y0 + r);
    pb.cubic_to(x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0);
    pb.close();
    pb.finish()
}

fn oval(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Path> {
    PathBuilder::from_oval(Rect::from_ltrb(x0, y0, x1, y1)?)
}

fn polygon(points: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);
    for p in &points[1..] {
        pb.line_to(p.0, p.1);
    }
    pb.close();
    pb.finish()
}

fn line(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x0, y0);
    pb.line_to(x1, y1);
    pb.finish()
}

/// Elliptical arc inside a box, angles in degrees clockwise from 3 o'clock (Pillow's convention).
fn arc(x0: f32, y0: f32, x1: f32, y1: f32, start: f32, end: f32) -> Option<Path> {
    let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let mut pb = PathBuilder::new();
    const STEPS: usize = 16;
    for i in 0..=STEPS {
        let a = (start + (end - start) * i as f32 / STEPS as f32).to_radians();
        let (x, y) = (cx + rx * a.cos(), cy + ry * a.sin());
        if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
    }
    pb.finish()
}

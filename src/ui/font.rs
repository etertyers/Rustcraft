//! The Minecraft font: vanilla's bitmap glyphs from the resource layers
//! (`font/default.json`'s bitmap providers, or the built-in layout of
//! `textures/font/ascii.png` when no layer has the JSON), turned into a
//! TrueType font of pixel squares so egui draws every piece of game text
//! with it. Widths are measured from each glyph's pixel columns, with
//! vanilla's 1 pixel of spacing, and a bold face draws each glyph again a
//! pixel to the right, as vanilla does. `§` codes color and format text,
//! and [`shadowed`] adds vanilla's drop shadow.

use std::collections::BTreeMap;

use egui::{
    Color32, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Painter, Pos2, TextFormat,
    TextStyle, text::LayoutJob, vec2,
};
use image::RgbaImage;
use log::{info, warn};

use crate::pack::Resources;

/// Font units per GUI pixel: the em is 8 pixels, the font's height.
const UNITS_PER_PIXEL: f32 = 128.0;
const UNITS_PER_EM: u16 = 8 * UNITS_PER_PIXEL as u16;
/// Pixels above the baseline, below it, and between lines (9 in all).
const ASCENT: f32 = 7.0;
const DESCENT: f32 = 1.0;
const LINE_GAP: f32 = 1.0;
/// The space's advance (vanilla's `include/space` provider).
const SPACE_ADVANCE: f32 = 4.0;

/// The family bold text is drawn in.
pub const BOLD: &str = "minecraft_bold";

/// One bitmap provider: a sheet of glyph cells, the characters in each row,
/// the glyphs' height in GUI pixels and how much of it is above the baseline.
#[derive(Debug, Clone)]
pub struct Provider {
    pub file: String,
    pub height: f32,
    pub ascent: f32,
    pub rows: Vec<Vec<char>>,
}

/// One glyph, in GUI pixels: its filled pixel rectangles (left, top above
/// the baseline, width, height) and its advance.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub rects: Vec<[f32; 4]>,
    pub advance: f32,
}

/// The rows of vanilla's `ascii.png` from the space on: ASCII, then the
/// code page 437 letters, lines and symbols (the same in 1.12 and 26.x).
/// The first two rows, which changed between versions, are left out.
const ASCII_ROWS: [&str; 14] = [
    " !\"#$%&'()*+,-./",
    "0123456789:;<=>?",
    "@ABCDEFGHIJKLMNO",
    "PQRSTUVWXYZ[\\]^_",
    "`abcdefghijklmno",
    "pqrstuvwxyz{|}~\0",
    "ÇüéâäàåçêëèïîìÄÅ",
    "ÉæÆôöòûùÿÖÜø£Ø×ƒ",
    "áíóúñÑªº¿®¬½¼¡«»",
    "░▒▓│┤╡╢╖╕╣║╗╝╜╛┐",
    "└┴┬├─┼╞╟╚╔╩╦╠═╬╧",
    "╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀",
    "αβΓπΣσμτΦΘΩδ∞∅∈∩",
    "≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\0",
];

/// The built-in provider used when no layer has `font/default.json`.
pub fn builtin_ascii() -> Provider {
    let mut rows = vec![vec!['\0'; 16]; 2];
    rows.extend(ASCII_ROWS.iter().map(|row| row.chars().collect()));
    Provider {
        file: "font/ascii".to_owned(),
        height: 8.0,
        ascent: 7.0,
        rows,
    }
}

/// The bitmap providers `font/default.json` lists, following `reference`
/// providers into `font/include/`.
pub fn providers(res: &Resources) -> Vec<Provider> {
    let mut out = Vec::new();
    collect(res, "font/default.json", &mut out, 0);
    if out.is_empty() {
        out.push(builtin_ascii());
    }
    out
}

fn collect(res: &Resources, path: &str, out: &mut Vec<Provider>, depth: u32) {
    let Some(bytes) = res.bytes(path) else {
        return;
    };
    let json: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(json) => json,
        Err(err) => {
            warn!("can't read {path}: {err}");
            return;
        }
    };
    let Some(list) = json.get("providers").and_then(|p| p.as_array()) else {
        return;
    };
    let strip = |id: &str| id.trim_start_matches("minecraft:").to_owned();
    for provider in list {
        match provider.get("type").and_then(|t| t.as_str()) {
            Some("reference") if depth < 4 => {
                if let Some(id) = provider.get("id").and_then(|i| i.as_str()) {
                    collect(res, &format!("font/{}.json", strip(id)), out, depth + 1);
                }
            }
            Some("bitmap") => {
                let Some(file) = provider.get("file").and_then(|f| f.as_str()) else {
                    continue;
                };
                let rows = provider
                    .get("chars")
                    .and_then(|c| c.as_array())
                    .map(|rows| {
                        rows.iter()
                            .filter_map(|row| row.as_str())
                            .map(|row| row.chars().collect())
                            .collect()
                    })
                    .unwrap_or_default();
                let number = |key: &str, default: f32| {
                    provider
                        .get(key)
                        .and_then(|v| v.as_f64())
                        .map_or(default, |v| v as f32)
                };
                out.push(Provider {
                    file: strip(file).trim_end_matches(".png").to_owned(),
                    height: number("height", 8.0),
                    ascent: number("ascent", 7.0),
                    rows,
                });
            }
            _ => {}
        }
    }
}

/// Cuts `sheet` into `provider`'s glyphs (vanilla's `BitmapProvider`):
/// a glyph is as wide as its rightmost column with a pixel in it, scaled
/// to the provider's height, plus 1 pixel of spacing.
pub fn glyphs(provider: &Provider, sheet: &RgbaImage, out: &mut BTreeMap<char, Glyph>) {
    let rows = provider.rows.len() as u32;
    let columns = provider.rows.iter().map(Vec::len).max().unwrap_or(0) as u32;
    if rows == 0 || columns == 0 {
        return;
    }
    let (cell_w, cell_h) = (sheet.width() / columns, sheet.height() / rows);
    if cell_w == 0 || cell_h == 0 {
        return;
    }
    let scale = provider.height / cell_h as f32;
    for (row, chars) in provider.rows.iter().enumerate() {
        for (column, &ch) in chars.iter().enumerate() {
            if ch == '\0' || ch == ' ' || out.contains_key(&ch) {
                continue;
            }
            let (x0, y0) = (column as u32 * cell_w, row as u32 * cell_h);
            let lit = |x: u32, y: u32| sheet.get_pixel(x0 + x, y0 + y)[3] > 0;
            let Some(width) = (0..cell_w).rev().find(|&x| (0..cell_h).any(|y| lit(x, y))) else {
                continue;
            };
            let mut rects = Vec::new();
            for y in 0..cell_h {
                let mut x = 0;
                while x <= width {
                    if !lit(x, y) {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x <= width && lit(x, y) {
                        x += 1;
                    }
                    rects.push([
                        start as f32 * scale,
                        provider.ascent - y as f32 * scale,
                        (x - start) as f32 * scale,
                        scale,
                    ]);
                }
            }
            let advance = ((width + 1) as f32 * scale + 0.5).floor() + 1.0;
            out.insert(ch, Glyph { rects, advance });
        }
    }
}

/// Every glyph the resource layers give, with the space.
pub fn load(res: &Resources) -> BTreeMap<char, Glyph> {
    let mut out = BTreeMap::new();
    out.insert(
        ' ',
        Glyph {
            rects: Vec::new(),
            advance: SPACE_ADVANCE,
        },
    );
    for provider in providers(res) {
        match res.image(&provider.file) {
            Some(sheet) => glyphs(&provider, &sheet, &mut out),
            None => warn!("no font sheet {}", provider.file),
        }
    }
    out
}

/// The bold face: each glyph drawn again a pixel to the right, one pixel
/// wider.
pub fn embolden(glyphs: &BTreeMap<char, Glyph>) -> BTreeMap<char, Glyph> {
    glyphs
        .iter()
        .map(|(&ch, glyph)| {
            let mut rects = glyph.rects.clone();
            rects.extend(glyph.rects.iter().map(|&[x, y, w, h]| [x + 1.0, y, w, h]));
            (
                ch,
                Glyph {
                    rects,
                    advance: glyph.advance + 1.0,
                },
            )
        })
        .collect()
}

/// A TrueType font with a square for each pixel rectangle of each glyph.
pub fn truetype(glyphs: &BTreeMap<char, Glyph>) -> Vec<u8> {
    let units = |px: f32| (px * UNITS_PER_PIXEL).round() as i16;
    // Glyph 0 is the empty .notdef; then one per character, in order.
    let mut glyf = Vec::new();
    // .notdef starts and ends at 0.
    let mut loca = vec![0u32, 0];
    let mut hmtx = vec![0u8; 4];
    let (mut max_points, mut max_contours, mut max_advance) = (0u16, 0u16, 0u16);
    let (mut x_min, mut y_min, mut x_max, mut y_max) = (0i16, 0i16, 0i16, 0i16);
    for glyph in glyphs.values() {
        let advance = units(glyph.advance) as u16;
        max_advance = max_advance.max(advance);
        let mut lsb = 0i16;
        if !glyph.rects.is_empty() {
            let corners: Vec<[i16; 4]> = glyph
                .rects
                .iter()
                .map(|&[x, top, w, h]| [units(x), units(top - h), units(x + w), units(top)])
                .collect();
            let bx0 = corners.iter().map(|c| c[0]).min().unwrap();
            let by0 = corners.iter().map(|c| c[1]).min().unwrap();
            let bx1 = corners.iter().map(|c| c[2]).max().unwrap();
            let by1 = corners.iter().map(|c| c[3]).max().unwrap();
            (x_min, y_min, x_max, y_max) = (
                x_min.min(bx0),
                y_min.min(by0),
                x_max.max(bx1),
                y_max.max(by1),
            );
            lsb = bx0;
            let contours = corners.len() as u16;
            max_contours = max_contours.max(contours);
            max_points = max_points.max(contours * 4);
            for v in [contours as i16, bx0, by0, bx1, by1] {
                glyf.extend(v.to_be_bytes());
            }
            for i in 0..contours {
                glyf.extend((i * 4 + 3).to_be_bytes());
            }
            glyf.extend(0u16.to_be_bytes());
            glyf.extend(std::iter::repeat_n(1u8, usize::from(contours) * 4));
            // Clockwise from the bottom left: up, right, down.
            let points: Vec<(i16, i16)> = corners
                .iter()
                .flat_map(|&[x0, y0, x1, y1]| [(x0, y0), (x0, y1), (x1, y1), (x1, y0)])
                .collect();
            let mut last = 0i16;
            for &(x, _) in &points {
                glyf.extend((x - last).to_be_bytes());
                last = x;
            }
            last = 0;
            for &(_, y) in &points {
                glyf.extend((y - last).to_be_bytes());
                last = y;
            }
            while !glyf.len().is_multiple_of(4) {
                glyf.push(0);
            }
        }
        loca.push(glyf.len() as u32);
        hmtx.extend(advance.to_be_bytes());
        hmtx.extend(lsb.to_be_bytes());
    }
    let glyph_count = glyphs.len() as u16 + 1;

    let mut cmap = Vec::new();
    for v in [0u16, 2, 0, 4] {
        cmap.extend(v.to_be_bytes());
    }
    cmap.extend(20u32.to_be_bytes());
    for v in [3u16, 10] {
        cmap.extend(v.to_be_bytes());
    }
    cmap.extend(20u32.to_be_bytes());
    let groups = glyphs.len() as u32;
    cmap.extend(12u16.to_be_bytes());
    cmap.extend(0u16.to_be_bytes());
    for v in [16 + 12 * groups, 0, groups] {
        cmap.extend(v.to_be_bytes());
    }
    for (i, &ch) in glyphs.keys().enumerate() {
        for v in [ch as u32, ch as u32, i as u32 + 1] {
            cmap.extend(v.to_be_bytes());
        }
    }

    let mut head = Vec::new();
    for v in [0x0001_0000u32, 0x0001_0000, 0, 0x5F0F_3CF5] {
        head.extend(v.to_be_bytes());
    }
    head.extend(0x000Bu16.to_be_bytes());
    head.extend(UNITS_PER_EM.to_be_bytes());
    head.extend([0u8; 16]);
    for v in [x_min, y_min, x_max, y_max] {
        head.extend(v.to_be_bytes());
    }
    for v in [0u16, 8, 2, 1, 0] {
        head.extend(v.to_be_bytes());
    }

    let mut hhea = Vec::new();
    hhea.extend(0x0001_0000u32.to_be_bytes());
    for v in [units(ASCENT), -units(DESCENT), units(LINE_GAP)] {
        hhea.extend(v.to_be_bytes());
    }
    hhea.extend(max_advance.to_be_bytes());
    for v in [0i16, 0, x_max, 1, 0, 0, 0, 0, 0, 0, 0] {
        hhea.extend(v.to_be_bytes());
    }
    hhea.extend(glyph_count.to_be_bytes());

    let mut maxp = Vec::new();
    maxp.extend(0x0001_0000u32.to_be_bytes());
    for v in [
        glyph_count,
        max_points,
        max_contours,
        0,
        0,
        2,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ] {
        maxp.extend(v.to_be_bytes());
    }

    let mut post = Vec::new();
    post.extend(0x0003_0000u32.to_be_bytes());
    post.extend([0u8; 28]);

    let loca: Vec<u8> = loca.iter().flat_map(|v| v.to_be_bytes()).collect();
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"cmap", cmap),
        (b"glyf", glyf),
        (b"head", head),
        (b"hhea", hhea),
        (b"hmtx", hmtx),
        (b"loca", loca),
        (b"maxp", maxp),
        (b"post", post),
    ];
    tables.sort_by_key(|(tag, _)| **tag);
    sfnt(&tables)
}

/// The font file: the table directory, then each table padded to 4 bytes.
fn sfnt(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let n = tables.len() as u16;
    let power = 1u16 << (15 - n.leading_zeros());
    let mut out = Vec::new();
    out.extend(0x0001_0000u32.to_be_bytes());
    for v in [
        n,
        power * 16,
        power.trailing_zeros() as u16,
        n * 16 - power * 16,
    ] {
        out.extend(v.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in tables {
        let mut padded = data.clone();
        while !padded.len().is_multiple_of(4) {
            padded.push(0);
        }
        let sum = padded
            .chunks(4)
            .map(|w| u32::from_be_bytes([w[0], w[1], w[2], w[3]]))
            .fold(0u32, u32::wrapping_add);
        out.extend(tag.iter());
        out.extend(sum.to_be_bytes());
        out.extend((offset as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        offset += padded.len();
        body.extend(padded);
    }
    out.extend(body);
    out
}

/// Makes the Minecraft font egui's first choice for all text (its own
/// fonts stay behind it for characters the bitmaps don't have), and sizes
/// the text styles in whole multiples of the font's 8 pixels.
pub fn install(ctx: &egui::Context, res: &Resources) {
    let glyphs = load(res);
    info!("font: {} glyphs", glyphs.len());
    let tweak = FontTweak {
        hinting: Some(false),
        ..Default::default()
    };
    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, faces) in [("minecraft", glyphs.clone()), (BOLD, embolden(&glyphs))] {
        fonts.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(FontData::from_owned(truetype(&faces)).tweak(tweak.clone())),
        );
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "minecraft".to_owned());
    }
    let mut bold = vec![BOLD.to_owned()];
    bold.extend(fallbacks);
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|style| {
        for (text_style, size) in [
            (TextStyle::Small, 8.0),
            (TextStyle::Body, 16.0),
            (TextStyle::Button, 16.0),
            (TextStyle::Monospace, 16.0),
            (TextStyle::Heading, 24.0),
        ] {
            if let Some(font) = style.text_styles.get_mut(&text_style) {
                font.size = size;
            }
        }
    });
}

/// Vanilla's 16 `§` colors.
const COLORS: [u32; 16] = [
    0x000000, 0x0000AA, 0x00AA00, 0x00AAAA, 0xAA0000, 0xAA00AA, 0xFFAA00, 0xAAAAAA, 0x555555,
    0x5555FF, 0x55FF55, 0x55FFFF, 0xFF5555, 0xFF55FF, 0xFFFF55, 0xFFFFFF,
];

/// What a `§` code sets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub color: Color32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

/// Splits `text` at its `§` codes: colors (`0`–`9`, `a`–`f`, which also
/// end the formats), bold `l`, italic `o`, underline `n`, strikethrough
/// `m` and reset `r`. `§k` (obfuscated) is dropped. Each run starts in
/// `color` unless a code says otherwise, and so does each new line (a
/// tooltip's lines are separate texts in vanilla).
pub fn runs(text: &str, color: Color32) -> Vec<(String, Style)> {
    let plain = Style {
        color,
        bold: false,
        italic: false,
        underline: false,
        strikethrough: false,
    };
    let mut style = plain;
    let mut out: Vec<(String, Style)> = Vec::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\n' {
            style = plain;
        }
        if ch == '§' {
            let Some(code) = chars.next() else { break };
            match code.to_ascii_lowercase() {
                c @ ('0'..='9' | 'a'..='f') => {
                    let c = COLORS[c.to_digit(16).unwrap() as usize];
                    style = Style {
                        color: Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8),
                        ..plain
                    };
                }
                'l' => style.bold = true,
                'o' => style.italic = true,
                'n' => style.underline = true,
                'm' => style.strikethrough = true,
                'r' => style = plain,
                _ => {}
            }
            continue;
        }
        match out.last_mut() {
            Some((run, last)) if *last == style => run.push(ch),
            _ => out.push((ch.to_string(), style)),
        }
    }
    out
}

/// `text` laid out with its `§` codes at `size`, every color dimmed to
/// vanilla's shadow (a quarter of it) when `shadow`.
pub fn job(text: &str, size: f32, color: Color32, shadow: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (run, style) in runs(text, color) {
        let color = if shadow {
            shadow_color(style.color)
        } else {
            style.color
        };
        let family = if style.bold {
            FontFamily::Name(BOLD.into())
        } else {
            FontFamily::Proportional
        };
        let line = |on: bool| {
            if on {
                egui::Stroke::new(size / 8.0, color)
            } else {
                egui::Stroke::NONE
            }
        };
        job.append(
            &run,
            0.0,
            TextFormat {
                font_id: FontId::new(size, family),
                color,
                italics: style.italic,
                underline: line(style.underline),
                strikethrough: line(style.strikethrough),
                ..Default::default()
            },
        );
    }
    job
}

/// Vanilla's text shadow: the color at a quarter of its brightness.
pub fn shadow_color(color: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r() / 4, color.g() / 4, color.b() / 4, color.a())
}

/// Paints `text` (with `§` codes) at `size` with vanilla's drop shadow a
/// font pixel right and down. Returns the text's size.
pub fn shadowed(
    painter: &Painter,
    at: Pos2,
    align: egui::Align2,
    text: &str,
    color: Color32,
    size: f32,
) -> egui::Vec2 {
    let pixel = size / 8.0;
    let front = painter.layout_job(job(text, size, color, false));
    let back = painter.layout_job(job(text, size, color, true));
    let rect = align.anchor_size(at, front.size());
    painter.galley(rect.min + vec2(pixel, pixel), back, Color32::WHITE);
    let size = front.size();
    painter.galley(rect.min, front, Color32::WHITE);
    size
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sheet of 16 × 16 cells of 8 pixels with an "A" 5 wide and an "i"
    /// 1 wide where ASCII puts them.
    fn sheet() -> RgbaImage {
        let mut image = RgbaImage::new(128, 128);
        let mut fill = |ch: u32, width: u32| {
            let (cx, cy) = ((ch % 16) * 8, (ch / 16) * 8);
            for y in 0..7 {
                for x in 0..width {
                    if (x + y) % 2 == 0 || width == 1 {
                        image.put_pixel(cx + x, cy + y, image::Rgba([255; 4]));
                    }
                }
            }
        };
        fill(b'A' as u32, 5);
        fill(b'i' as u32, 1);
        image
    }

    #[test]
    fn widths_come_from_the_pixel_columns_plus_one() {
        let mut glyphs = BTreeMap::new();
        super::glyphs(&builtin_ascii(), &sheet(), &mut glyphs);
        assert_eq!(glyphs[&'A'].advance, 6.0);
        assert_eq!(glyphs[&'i'].advance, 2.0);
        assert!(!glyphs.contains_key(&'B'), "empty cells are left out");
        // The top row sits at the ascent; a 1-wide column is 7 squares.
        assert_eq!(glyphs[&'i'].rects.len(), 7);
        assert_eq!(glyphs[&'i'].rects[0], [0.0, 7.0, 1.0, 1.0]);
        let bold = embolden(&glyphs);
        assert_eq!(bold[&'A'].advance, 7.0);
    }

    #[test]
    fn a_larger_sheet_scales_down_to_eight_pixels() {
        let small = sheet();
        let big = image::imageops::resize(&small, 256, 256, image::imageops::FilterType::Nearest);
        let mut glyphs = BTreeMap::new();
        super::glyphs(&builtin_ascii(), &big, &mut glyphs);
        assert_eq!(glyphs[&'A'].advance, 6.0);
        // Each pixel is now four half-pixel ones; a row's run is still 1 wide.
        assert_eq!(glyphs[&'i'].rects[0], [0.0, 7.0, 1.0, 0.5]);
    }

    #[test]
    fn egui_lays_text_out_in_the_font_at_its_pixel_widths() {
        let mut glyphs = BTreeMap::new();
        glyphs.insert(
            ' ',
            Glyph {
                rects: Vec::new(),
                advance: SPACE_ADVANCE,
            },
        );
        super::glyphs(&builtin_ascii(), &sheet(), &mut glyphs);
        let ctx = egui::Context::default();
        let mut fonts = FontDefinitions::empty();
        fonts.font_data.insert(
            "minecraft".into(),
            std::sync::Arc::new(FontData::from_owned(truetype(&glyphs))),
        );
        fonts
            .families
            .insert(FontFamily::Proportional, vec!["minecraft".into()]);
        ctx.set_fonts(fonts);
        ctx.run_ui(Default::default(), |_| {})
            .textures_delta
            .clear();
        let width = |text: &str, size: f32| {
            ctx.fonts_mut(|f| {
                f.layout_no_wrap(text.into(), FontId::proportional(size), Color32::WHITE)
                    .size()
            })
        };
        // A, i, space, A: 6 + 2 + 4 + 6 pixels; lines are 9 tall.
        let size = width("Ai A", 8.0);
        assert!((size.x - 18.0).abs() < 0.01, "{size:?}");
        assert!((size.y - 9.0).abs() < 0.01, "{size:?}");
        assert!((width("Ai A", 16.0).x - 36.0).abs() < 0.01);
        ctx.run_ui(Default::default(), |_| {})
            .textures_delta
            .clear();
    }

    #[test]
    fn section_codes_color_and_format() {
        let white = Color32::WHITE;
        let runs = runs("§cRed §lBold§r plain §x?", white);
        assert_eq!(runs.len(), 3, "{runs:?}");
        assert_eq!(runs[0].0, "Red ");
        assert_eq!(runs[0].1.color, Color32::from_rgb(0xFF, 0x55, 0x55));
        assert!(runs[1].1.bold && runs[1].1.color == runs[0].1.color);
        assert_eq!(runs[2].0, " plain ?");
        assert_eq!(runs[2].1.color, white);
        assert!(!runs[2].1.bold);
        // A new line starts plain.
        let lines = super::runs("§6§lName\nSharpness", white);
        assert_eq!(lines.last().unwrap().1.color, white);
        assert!(!lines.last().unwrap().1.bold);
        // A color ends bold.
        assert!(!super::runs("§lA§aB", white)[1].1.bold);
        assert_eq!(
            shadow_color(Color32::from_rgb(200, 100, 40)),
            Color32::from_rgb(50, 25, 10)
        );
    }

    /// Reads back the box of the glyph the font maps `ch` to.
    fn glyph_box(font: &[u8], ch: char) -> [i16; 4] {
        let u16_at = |at: usize| u16::from_be_bytes([font[at], font[at + 1]]);
        let u32_at = |at: usize| u32::from_be_bytes(font[at..at + 4].try_into().unwrap());
        let table = |tag: &[u8; 4]| {
            (0..u16_at(4) as usize)
                .map(|i| 12 + 16 * i)
                .find(|&r| &font[r..r + 4] == tag)
                .map(|r| u32_at(r + 8) as usize)
                .unwrap()
        };
        let (cmap, loca, glyf) = (table(b"cmap"), table(b"loca"), table(b"glyf"));
        let sub = cmap + u32_at(cmap + 8) as usize;
        let glyph = (0..u32_at(sub + 12) as usize)
            .map(|g| sub + 16 + 12 * g)
            .find(|&g| u32_at(g) == ch as u32)
            .map(|g| u32_at(g + 8) as usize)
            .unwrap();
        let at = glyf + u32_at(loca + 4 * glyph) as usize;
        std::array::from_fn(|i| u16_at(at + 2 + 2 * i) as i16)
    }

    #[test]
    fn each_character_maps_to_its_own_outline() {
        let mut glyphs = BTreeMap::new();
        super::glyphs(&builtin_ascii(), &sheet(), &mut glyphs);
        let font = truetype(&glyphs);
        // 'A' is 5 pixels wide, 'i' 1; both 7 tall from the baseline.
        assert_eq!(glyph_box(&font, 'A'), [0, 0, 640, 896]);
        assert_eq!(glyph_box(&font, 'i'), [0, 0, 128, 896]);
    }
}

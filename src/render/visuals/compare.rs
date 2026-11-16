//! `--shot <name> --compare` (M54): renders the scene once per look, each
//! in its own run of the game (scenes are staged the same every run, so
//! it is the same frame), and writes one PNG with Vanilla on the left and
//! Trailer on the right, labeled.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};

use super::{Quality, Style};

/// What `--compare` was asked to show: (style, shaders on) pairs, and how
/// many go across.
pub fn plan(spec: &str) -> Result<(Vec<(Style, bool)>, usize)> {
    match spec.trim().to_ascii_lowercase().as_str() {
        // Styles across, shaders off on top and on below.
        "all" => Ok((
            [false, true]
                .into_iter()
                .flat_map(|on| {
                    [Style::Vanilla, Style::Barebones, Style::Realistic].map(|s| (s, on))
                })
                .collect(),
            3,
        )),
        // The old comparison: Vanilla (no shaders) beside the Trailer look.
        "" => Ok((vec![(Style::Vanilla, false), (Style::Barebones, true)], 2)),
        list => {
            let mut pairs = Vec::new();
            for item in list.split(',') {
                let (style, shaders) = item.split_once('/').unwrap_or((item, "off"));
                let style = Style::parse(style)
                    .with_context(|| format!("bad style in --compare {item:?}"))?;
                let on = match shaders {
                    "on" => true,
                    "off" => false,
                    other => bail!("bad shaders {other:?} in --compare (on or off)"),
                };
                pairs.push((style, on));
            }
            let columns = pairs.len().min(3);
            Ok((pairs, columns))
        }
    }
}

/// "BAREBONES - SHADERS ON" (letters only: the label font has no digits).
pub fn caption(style: Style, shaders: bool, quality: Quality) -> String {
    let mut text = style.name().to_uppercase();
    text += if shaders {
        " - SHADERS ON"
    } else {
        " - SHADERS OFF"
    };
    if shaders {
        text += &format!(" ({})", quality.name().to_uppercase());
    }
    text
}

/// Runs each pair in its own game and joins the pictures into `out`.
pub fn run(args: &[String], spec: &str, out: &Path, quality: Option<Quality>) -> Result<()> {
    let exe = crate::platform::env::current_exe()?;
    // This command line minus --compare (and what it names), --out,
    // --style, --shaders, --visuals and --quality.
    let mut rest = Vec::new();
    let mut words = args.iter().skip(1).peekable();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--compare" => {
                if words.peek().is_some_and(|next| !next.starts_with("--")) {
                    words.next();
                }
            }
            "--out" | "--style" | "--shaders" | "--visuals" | "--quality" => {
                words.next();
            }
            _ => rest.push(word.clone()),
        }
    }
    let quality = quality.unwrap_or_default();
    let (pairs, columns) = plan(spec)?;
    let dir = crate::platform::env::temp_dir().join(format!(
        "rustcraft-compare-{}",
        crate::platform::env::process_id()
    ));
    crate::platform::fs::create_dir_all(&dir)?;
    let mut cells = Vec::new();
    for (i, &(style, on)) in pairs.iter().enumerate() {
        let path = dir.join(format!("{i}.png"));
        let status = Command::new(&exe)
            .args(&rest)
            .args([
                "--style",
                style.key(),
                "--shaders",
                if on { "on" } else { "off" },
                "--quality",
                quality.name(),
                "--out",
            ])
            .arg(&path)
            .status()?;
        if !status.success() {
            bail!("the {} run failed: {status}", caption(style, on, quality));
        }
        cells.push((
            caption(style, on, quality),
            image::open(&path)
                .with_context(|| format!("reading {}", path.display()))?
                .to_rgba8(),
        ));
    }
    let joined = if cells.len() == 2 && columns == 2 {
        side_by_side(&cells[0].1, &cells[1].1, &cells[0].0, &cells[1].0)
    } else {
        grid(&cells, columns)
    };
    if let Some(parent) = out.parent() {
        crate::platform::fs::create_dir_all(parent)?;
    }
    joined
        .save(out)
        .with_context(|| format!("saving {}", out.display()))?;
    let _ = crate::platform::fs::remove_dir_all(&dir);
    println!("{}", out.display());
    Ok(())
}

/// The pictures in rows of `columns`, a 4-pixel black seam between, each
/// labeled at its top left.
pub fn grid(cells: &[(String, RgbaImage)], columns: usize) -> RgbaImage {
    const SEAM: u32 = 4;
    let columns = columns.max(1);
    let rows = cells.len().div_ceil(columns);
    let (w, h) = cells
        .first()
        .map_or((1, 1), |(_, image)| image.dimensions());
    let mut out = RgbaImage::from_pixel(
        columns as u32 * w + (columns as u32 - 1) * SEAM,
        rows as u32 * h + (rows as u32).saturating_sub(1) * SEAM,
        Rgba([0, 0, 0, 255]),
    );
    let scale = (h / 180).max(2);
    for (i, (text, image)) in cells.iter().enumerate() {
        let x = (i % columns) as u32 * (w + SEAM);
        let y = (i / columns) as u32 * (h + SEAM);
        image::imageops::replace(&mut out, image, i64::from(x), i64::from(y));
        label(&mut out, text, x + 8, y + 8, scale);
    }
    out
}

/// `left` and `right` next to each other with a 4-pixel black seam, each
/// labeled at its top left.
pub fn side_by_side(left: &RgbaImage, right: &RgbaImage, a: &str, b: &str) -> RgbaImage {
    const SEAM: u32 = 4;
    let height = left.height().max(right.height());
    let mut out = RgbaImage::from_pixel(
        left.width() + SEAM + right.width(),
        height,
        Rgba([0, 0, 0, 255]),
    );
    image::imageops::replace(&mut out, left, 0, 0);
    image::imageops::replace(&mut out, right, i64::from(left.width() + SEAM), 0);
    let scale = (height / 180).max(2);
    label(&mut out, a, 8, 8, scale);
    label(&mut out, b, left.width() + SEAM + 8, 8, scale);
    out
}

/// Writes `text` (capitals, digits, spaces and brackets) in white on a
/// dark box, each font pixel `scale` pixels wide.
fn label(image: &mut RgbaImage, text: &str, x: u32, y: u32, scale: u32) {
    let width = text.chars().count() as u32 * 6 * scale + 2 * scale;
    let height = 9 * scale;
    for py in y..(y + height).min(image.height()) {
        for px in x..(x + width).min(image.width()) {
            let p = image.get_pixel_mut(px, py);
            for c in 0..3 {
                p.0[c] /= 3;
            }
        }
    }
    for (i, c) in text.chars().enumerate() {
        let rows = glyph(c);
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                let gx = x + scale + (i as u32 * 6 + col) * scale;
                let gy = y + scale + row as u32 * scale;
                for dy in 0..scale {
                    for dx in 0..scale {
                        if gx + dx < image.width() && gy + dy < image.height() {
                            image.put_pixel(gx + dx, gy + dy, Rgba([255, 255, 255, 255]));
                        }
                    }
                }
            }
        }
    }
}

/// A 5 × 7 glyph, a row per byte (bit 4 is the left column).
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        'D' => [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
        'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        'G' => [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f],
        'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'I' => [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0c],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '+' => [0x00, 0x04, 0x04, 0x1f, 0x04, 0x04, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x00],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_all_is_three_styles_across_with_shaders_off_then_on() {
        let (pairs, columns) = plan("all").unwrap();
        assert_eq!(columns, 3);
        assert_eq!(pairs.len(), 6);
        assert_eq!(pairs[0], (Style::Vanilla, false));
        assert_eq!(pairs[2], (Style::Realistic, false));
        assert_eq!(pairs[3], (Style::Vanilla, true));
        assert_eq!(pairs[5], (Style::Realistic, true));
        let (list, _) = plan("barebones/on,realistic/off").unwrap();
        assert_eq!(list, [(Style::Barebones, true), (Style::Realistic, false)]);
        assert!(plan("fancy/on").is_err());
        assert!(plan("vanilla/maybe").is_err());
        assert_eq!(
            caption(Style::Realistic, true, Quality::Medium),
            "REALISTIC - SHADERS ON (MEDIUM)"
        );
    }

    /// `--compare all` makes one 3×2 picture, each cell labeled.
    #[test]
    fn a_grid_of_six_is_three_across_and_two_down_with_labels() {
        let cell = |c: u8| {
            (
                format!("CELL {c}"),
                RgbaImage::from_pixel(120, 60, Rgba([c, c, c, 255])),
            )
        };
        let cells: Vec<_> = (1..=6).map(|c| cell(c * 30)).collect();
        let out = grid(&cells, 3);
        assert_eq!(out.dimensions(), (3 * 120 + 2 * 4, 2 * 60 + 4));
        assert_eq!(out.get_pixel(60, 55).0, [30, 30, 30, 255]);
        assert_eq!(out.get_pixel(124 + 60, 55).0, [60, 60, 60, 255]);
        assert_eq!(out.get_pixel(60, 64 + 55).0, [120, 120, 120, 255]);
        assert_eq!(out.get_pixel(2 * 124 + 60, 64 + 55).0, [180, 180, 180, 255]);
        let white =
            (0..120).any(|x| (0..30).any(|y| out.get_pixel(x, y).0 == [255, 255, 255, 255]));
        assert!(white, "labeled");
    }

    #[test]
    fn the_halves_sit_side_by_side_with_labels() {
        let left = RgbaImage::from_pixel(100, 60, Rgba([200, 0, 0, 255]));
        let right = RgbaImage::from_pixel(100, 60, Rgba([0, 0, 200, 255]));
        let out = side_by_side(&left, &right, "VANILLA", "TRAILER");
        assert_eq!(out.dimensions(), (204, 60));
        // Away from the labels each half is untouched.
        assert_eq!(out.get_pixel(50, 50).0, [200, 0, 0, 255]);
        assert_eq!(out.get_pixel(150, 50).0, [0, 0, 200, 255]);
        assert_eq!(out.get_pixel(101, 30).0, [0, 0, 0, 255]);
        // Some label pixels are white on each side.
        let white = |x0: u32, x1: u32| {
            (x0..x1).any(|x| (0..30).any(|y| out.get_pixel(x, y).0 == [255, 255, 255, 255]))
        };
        assert!(white(0, 100) && white(104, 204));
    }
}

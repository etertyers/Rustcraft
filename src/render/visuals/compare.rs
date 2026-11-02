//! `--shot <name> --compare` (M54): renders the scene once per look, each
//! in its own run of the game (scenes are staged the same every run, so
//! it is the same frame), and writes one PNG with Vanilla on the left and
//! Trailer on the right, labeled.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};

use super::Quality;

/// Runs the two looks and joins them into `out`.
pub fn run(args: &[String], out: &Path, quality: Option<Quality>) -> Result<()> {
    let exe = std::env::current_exe()?;
    // This command line minus --compare, --out, --visuals and --quality.
    let mut rest = Vec::new();
    let mut words = args.iter().skip(1);
    while let Some(word) = words.next() {
        match word.as_str() {
            "--compare" => {}
            "--out" | "--visuals" | "--quality" => {
                words.next();
            }
            _ => rest.push(word.clone()),
        }
    }
    let quality = quality.unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("rustcraft-compare-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut halves = Vec::new();
    for look in ["vanilla", "trailer"] {
        let path = dir.join(format!("{look}.png"));
        let status = Command::new(&exe)
            .args(&rest)
            .args(["--visuals", look, "--quality", quality.name(), "--out"])
            .arg(&path)
            .status()?;
        if !status.success() {
            bail!("the {look} run failed: {status}");
        }
        halves.push(
            image::open(&path)
                .with_context(|| format!("reading {}", path.display()))?
                .to_rgba8(),
        );
    }
    let label = format!("TRAILER ({})", quality.name().to_uppercase());
    let joined = side_by_side(&halves[0], &halves[1], "VANILLA", &label);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    joined
        .save(out)
        .with_context(|| format!("saving {}", out.display()))?;
    let _ = std::fs::remove_dir_all(&dir);
    println!("{}", out.display());
    Ok(())
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

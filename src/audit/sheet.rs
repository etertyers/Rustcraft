//! Contact sheets: up to 256 labeled thumbnails to a picture, with a
//! colored border on anything a detector flagged. Labels use a tiny 3×5
//! pixel font so no font file is needed (the audit has no window).

use image::{Rgba, RgbaImage};

use super::detect::Detector;

/// Thumbnails per sheet side: 16×16 = 256 to a sheet.
pub const COLUMNS: u32 = 16;
pub const PER_SHEET: usize = (COLUMNS * COLUMNS) as usize;
/// A thumbnail's side, and the label's height under it.
pub const THUMB: u32 = 64;
const LABEL: u32 = 12;
const CELL_W: u32 = THUMB + 4;
const CELL_H: u32 = THUMB + LABEL + 4;
const BACKGROUND: Rgba<u8> = Rgba([34, 34, 40, 255]);
const LABEL_COLOR: Rgba<u8> = Rgba([210, 210, 210, 255]);

/// 3×5 glyphs, three bits a row (the top bit is the left column).
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_lowercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 7, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 2, 2, 2],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'a' => [2, 5, 7, 5, 5],
        'b' => [6, 5, 6, 5, 6],
        'c' => [3, 4, 4, 4, 3],
        'd' => [6, 5, 5, 5, 6],
        'e' => [7, 4, 6, 4, 7],
        'f' => [7, 4, 6, 4, 4],
        'g' => [3, 4, 5, 5, 3],
        'h' => [5, 5, 7, 5, 5],
        'i' => [7, 2, 2, 2, 7],
        'j' => [1, 1, 1, 5, 2],
        'k' => [5, 5, 6, 5, 5],
        'l' => [4, 4, 4, 4, 7],
        'm' => [5, 7, 7, 5, 5],
        'n' => [6, 5, 5, 5, 5],
        'o' => [2, 5, 5, 5, 2],
        'p' => [6, 5, 6, 4, 4],
        'q' => [2, 5, 5, 6, 3],
        'r' => [6, 5, 6, 5, 5],
        's' => [3, 4, 2, 1, 6],
        't' => [7, 2, 2, 2, 2],
        'u' => [5, 5, 5, 5, 7],
        'v' => [5, 5, 5, 5, 2],
        'w' => [5, 5, 7, 7, 5],
        'x' => [5, 5, 2, 5, 5],
        'y' => [5, 5, 2, 2, 2],
        'z' => [7, 1, 2, 4, 7],
        '_' => [0, 0, 0, 0, 7],
        '-' => [0, 0, 7, 0, 0],
        '.' => [0, 0, 0, 0, 2],
        '/' => [1, 1, 2, 4, 4],
        '#' => [5, 7, 5, 7, 5],
        ' ' => [0; 5],
        _ => [7, 5, 2, 0, 2],
    }
}

/// Draws `text` at (x, y) in the label font, 4 pixels a character.
pub fn draw_text(image: &mut RgbaImage, x: u32, y: u32, text: &str, color: Rgba<u8>) {
    for (i, c) in text.chars().enumerate() {
        for (row, bits) in glyph(c).into_iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    let (px, py) = (x + i as u32 * 4 + col, y + row as u32);
                    if px < image.width() && py < image.height() {
                        image.put_pixel(px, py, color);
                    }
                }
            }
        }
    }
}

/// The border color of a detector.
pub fn border_color(detector: Detector) -> Rgba<u8> {
    match detector {
        Detector::Placeholder | Detector::UnknownTexture => Rgba([255, 0, 255, 255]),
        Detector::TransparentFace | Detector::TransparentPart => Rgba([255, 60, 60, 255]),
        Detector::UvOutOfRange => Rgba([255, 150, 0, 255]),
        Detector::SizeMismatch | Detector::SquashedStrip => Rgba([255, 230, 0, 255]),
        Detector::Tint => Rgba([0, 200, 80, 255]),
        Detector::FlatIcon => Rgba([0, 200, 255, 255]),
        Detector::Orientation | Detector::DuplicateSides => Rgba([120, 120, 255, 255]),
    }
}

/// One cell of a sheet.
pub struct Cell<'a> {
    pub id: &'a str,
    pub image: &'a RgbaImage,
    /// The most severe flag's detector, for the border.
    pub flag: Option<Detector>,
}

/// The shortened label: the last path part, cut to what fits.
pub fn label(id: &str) -> String {
    let last = id.rsplit('/').next().unwrap_or(id);
    last.chars().take((CELL_W / 4) as usize).collect()
}

/// Lays `cells` out in a sheet, 16 to a row.
pub fn compose(cells: &[Cell]) -> RgbaImage {
    let rows = (cells.len() as u32).div_ceil(COLUMNS).max(1);
    let mut sheet = RgbaImage::from_pixel(COLUMNS * CELL_W, rows * CELL_H, BACKGROUND);
    for (i, cell) in cells.iter().enumerate() {
        let (cx, cy) = ((i as u32 % COLUMNS) * CELL_W, (i as u32 / COLUMNS) * CELL_H);
        if let Some(detector) = cell.flag {
            let color = border_color(detector);
            for x in 0..CELL_W - 2 {
                for t in 0..2 {
                    sheet.put_pixel(cx + 1 + x, cy + 1 + t, color);
                    sheet.put_pixel(cx + 1 + x, cy + THUMB + 1 - t + 2, color);
                }
            }
            for y in 0..THUMB + 4 {
                for t in 0..2 {
                    sheet.put_pixel(cx + 1 + t, cy + 1 + y, color);
                    sheet.put_pixel(cx + CELL_W - 3 + t, cy + 1 + y, color);
                }
            }
        }
        // The picture, centered (smaller ones keep their size).
        let (w, h) = cell.image.dimensions();
        let (ox, oy) = (
            cx + 2 + THUMB.saturating_sub(w) / 2,
            cy + 2 + THUMB.saturating_sub(h) / 2,
        );
        for y in 0..h.min(THUMB) {
            for x in 0..w.min(THUMB) {
                let p = cell.image.get_pixel(x, y);
                if p.0[3] > 0 {
                    sheet.put_pixel(ox + x, oy + y, *p);
                }
            }
        }
        draw_text(
            &mut sheet,
            cx + 2,
            cy + THUMB + 5,
            &label(cell.id),
            LABEL_COLOR,
        );
    }
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_holds_256_cells_and_marks_flagged_ones_with_a_border() {
        let thumb = RgbaImage::from_pixel(THUMB, THUMB, Rgba([10, 200, 10, 255]));
        let cells: Vec<Cell> = (0..300)
            .map(|i| Cell {
                id: "block/stone",
                image: &thumb,
                flag: (i == 1).then_some(Detector::Placeholder),
            })
            .collect();
        let sheet = compose(&cells[..PER_SHEET]);
        assert_eq!(sheet.width(), COLUMNS * CELL_W);
        assert_eq!(sheet.height(), COLUMNS * CELL_H);
        // Cell 1's border is the placeholder's magenta; cell 0 has none.
        assert_eq!(sheet.get_pixel(CELL_W + 1, 1).0, [255, 0, 255, 255]);
        assert_eq!(sheet.get_pixel(1, 1).0, BACKGROUND.0);
        // The thumbnail itself is drawn inside.
        assert_eq!(sheet.get_pixel(CELL_W + 20, 20).0, [10, 200, 10, 255]);
    }

    #[test]
    fn labels_are_the_last_path_part_cut_to_fit() {
        assert_eq!(label("block/stone"), "stone");
        assert_eq!(
            label("entity/pig/temperate_pig_long_name_here"),
            "temperate_pig_lon"
        );
    }
}

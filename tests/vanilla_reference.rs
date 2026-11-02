//! The Vanilla look is untouchable (v1.4): six `--shot` scenes rendered with
//! Visuals set to Vanilla must match the reference images rendered at M53
//! (tests/reference/vanilla/, rendered in M54's Step 0 before anything in
//! the renderer changed): at least 99.9% of pixels within 1/255 per channel.
//!
//! Each scene runs the game binary in a fresh, empty game folder (default
//! options, no resource packs) at 640 × 360. Needs a GPU; skipped on CI.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The scenes, all on seed 12345.
pub const SCENES: [&str; 6] = [
    "forest-noon",
    "lake-sunset",
    "cave-torches",
    "nether-wastes",
    "end-island",
    "underwater-ocean",
];

/// Share of pixels that must be within `TOLERANCE` per channel.
const MIN_MATCHING: f64 = 0.999;
const TOLERANCE: u8 = 1;

/// The share of pixels of `a` and `b` that are within `TOLERANCE` on
/// every channel (0 if the sizes differ).
fn matching(a: &image::RgbaImage, b: &image::RgbaImage) -> f64 {
    if a.dimensions() != b.dimensions() {
        return 0.0;
    }
    let close = a
        .pixels()
        .zip(b.pixels())
        .filter(|(p, q)| (0..3).all(|c| p.0[c].abs_diff(q.0[c]) <= TOLERANCE))
        .count();
    close as f64 / (a.width() * a.height()) as f64
}

/// Renders `scene` with `--visuals vanilla` into `out`.
fn render(scene: &str, folder: &Path, out: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(folder);
    let output = Command::new(env!("CARGO_BIN_EXE_rustcraft"))
        .args([
            "--shot",
            scene,
            "--size",
            "640x360",
            "--visuals",
            "vanilla",
            "--out",
        ])
        .arg(out)
        .env("RUSTCRAFT_SAVES", folder.join("saves"))
        .env("RUST_LOG", "rustcraft=info")
        .output()
        .map_err(|err| err.to_string())?;
    let log = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(log.into_owned());
    }
    // The window's scale factor sets the GUI's size, and the references
    // were rendered at 2 (a Retina display). Another display can't match.
    let scale = log
        .lines()
        .find_map(|l| l.split("scale factor ").nth(1))
        .and_then(|n| n.trim().parse::<f64>().ok());
    if scale != Some(REFERENCE_SCALE) {
        return Err(format!("{WRONG_DISPLAY}{scale:?}"));
    }
    Ok(())
}

/// The window scale factor the reference images were rendered at.
const REFERENCE_SCALE: f64 = 2.0;
/// Starts the message of a run on a display of another scale.
const WRONG_DISPLAY: &str = "skipped: the display's scale factor isn't 2: ";

/// Renders `scene` and compares it with its reference.
fn check(root: &Path, work: &Path, scene: &str) -> Result<(), String> {
    let out = work.join(format!("{scene}.png"));
    let folder = work.join(format!("{scene}-game"));
    render(scene, &folder, &out).map_err(|err| format!("{scene}: the game failed: {err}"))?;
    let reference = root.join(format!("tests/reference/vanilla/{scene}.png"));
    let want = image::open(&reference).expect("reference image").to_rgba8();
    let got = image::open(&out)
        .map_err(|e| format!("{scene}: {e}"))?
        .to_rgba8();
    let share = matching(&want, &got);
    println!("{scene}: {:.4}% of pixels match", share * 100.0);
    if share < MIN_MATCHING {
        return Err(format!(
            "{scene}: only {:.4}% of pixels match (kept {})",
            share * 100.0,
            out.display()
        ));
    }
    Ok(())
}

#[test]
fn vanilla_look_matches_the_m53_reference_images() {
    if std::env::var_os("CI").is_some() {
        eprintln!("skipped: the reference scenes need a GPU and a window (not on CI)");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work: PathBuf = std::env::temp_dir().join(format!("rustcraft-ref-{}", std::process::id()));
    let mut failures = Vec::new();
    let mut skipped = 0;
    for scene in SCENES {
        // A scene that fails or differs is rendered once more, and says
        // why (a game run can fail when the window server is busy).
        let mut result = check(root, &work, scene);
        if let Err(first) = &result {
            println!("{scene}: retrying after: {first}");
            result = check(root, &work, scene);
        }
        match result {
            Err(err) if err.contains(WRONG_DISPLAY) => {
                eprintln!("{scene}: {err}");
                skipped += 1;
            }
            Err(err) => failures.push(err),
            Ok(()) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    if skipped > 0 {
        eprintln!(
            "{skipped} scenes skipped: this display isn't the Retina scale the references need"
        );
    }
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn the_pixel_comparison_counts_close_pixels() {
    let a = image::RgbaImage::from_pixel(10, 10, image::Rgba([100, 100, 100, 255]));
    let mut b = a.clone();
    b.put_pixel(0, 0, image::Rgba([101, 99, 100, 255]));
    assert_eq!(matching(&a, &b), 1.0);
    b.put_pixel(1, 0, image::Rgba([102, 100, 100, 255]));
    assert!((matching(&a, &b) - 0.99).abs() < 1e-9);
    assert_eq!(matching(&a, &image::RgbaImage::new(5, 5)), 0.0);
}

/// `RUSTCRAFT_COMPARE=<dir a>:<dir b>`: prints how closely two folders of
/// renders match (for checking that scenes render the same every run).
#[test]
fn compare_two_folders_when_asked() {
    let Some(dirs) = std::env::var("RUSTCRAFT_COMPARE").ok() else {
        return;
    };
    let (a, b) = dirs.split_once(':').expect("<dir a>:<dir b>");
    for scene in SCENES {
        let load = |dir: &str| {
            image::open(Path::new(dir).join(format!("{scene}.png"))).map(|i| i.to_rgba8())
        };
        if let (Ok(x), Ok(y)) = (load(a), load(b)) {
            let (mut lo, mut hi) = ((u32::MAX, u32::MAX), (0, 0));
            for (px, py, p) in x.enumerate_pixels() {
                let q = y.get_pixel(px, py);
                if (0..3).any(|c| p.0[c].abs_diff(q.0[c]) > TOLERANCE) {
                    lo = (lo.0.min(px), lo.1.min(py));
                    hi = (hi.0.max(px), hi.1.max(py));
                }
            }
            println!(
                "{scene}: {:.4}% (differences in {lo:?}..{hi:?})",
                matching(&x, &y) * 100.0
            );
        }
    }
}

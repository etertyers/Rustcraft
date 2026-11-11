//! Starting safely (the v1.5 checkpoint L fixes): one place that checks
//! every texture against the GPU's limit, and loading the look and the GUI
//! images so that one bad resource pack can't stop the game from starting.

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow};
use image::RgbaImage;

use crate::{entity::mobs::MobRegistry, look::Look, options::Options, ui::gui::GuiTextures};

// ---- Texture limits -------------------------------------------------------

/// What wgpu falls back to if no adapter can be asked.
const FALLBACK_LIMIT: u32 = 8192;

static GPU_LIMIT: AtomicU32 = AtomicU32::new(FALLBACK_LIMIT);
static PACKS: Mutex<String> = Mutex::new(String::new());

/// The largest texture side the GPU takes (its adapter's
/// `max_texture_dimension_2d`), once `probe_gpu_limit` or the renderer has
/// set it.
pub fn gpu_limit() -> u32 {
    GPU_LIMIT.load(Ordering::Relaxed)
}

pub fn set_gpu_limit(side: u32) {
    GPU_LIMIT.store(side.max(256), Ordering::Relaxed);
}

/// Asks the first adapter for its limit (before the window's surface
/// exists, so the GUI can be told it too) and remembers it.
pub fn probe_gpu_limit() -> u32 {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    if let Ok(adapter) =
        crate::platform::task::block_on(instance.request_adapter(&Default::default()))
    {
        set_gpu_limit(adapter.limits().max_texture_dimension_2d);
    }
    gpu_limit()
}

/// The packs the textures come from, for the warnings.
pub fn set_pack_label(ids: &[String]) {
    if let Ok(mut label) = PACKS.lock() {
        *label = ids.join(", ");
    }
}

/// The whole-number factor that brings a `width` × `height` picture within
/// `limit`: 1 if it already fits.
pub fn shrink_factor(width: u32, height: u32, limit: u32) -> u32 {
    width.max(height).div_ceil(limit.max(1)).max(1)
}

/// `image` as it goes to the GPU or egui: itself if it fits `limit`, else
/// shrunk by a whole-number factor with nearest sampling (so its pixels stay
/// pixels). Logs a warning naming the texture and the packs. The UVs of
/// anything drawn from it are 0..1, so they stay right.
pub fn fit<'a>(name: &str, image: &'a RgbaImage, limit: u32) -> Cow<'a, RgbaImage> {
    let factor = shrink_factor(image.width(), image.height(), limit);
    if factor == 1 {
        return Cow::Borrowed(image);
    }
    let (w, h) = (
        image.width().div_ceil(factor),
        image.height().div_ceil(factor),
    );
    let packs = PACKS.lock().map(|p| p.clone()).unwrap_or_default();
    log::warn!(
        "texture {name:?} is {}x{}, over the limit of {limit}: shrinking it {factor}x to {w}x{h} (resource packs: {})",
        image.width(),
        image.height(),
        if packs.is_empty() { "none" } else { &packs },
    );
    Cow::Owned(image::imageops::resize(
        image,
        w,
        h,
        image::imageops::FilterType::Nearest,
    ))
}

/// A picture for the GPU: within its limit.
pub fn fit_gpu<'a>(name: &str, image: &'a RgbaImage, limit: u32) -> Cow<'a, RgbaImage> {
    fit(name, image, limit)
}

/// An egui context that knows the GPU's limit (not egui's default 2048)
/// before any texture is loaded: one empty pass hands it to the context.
/// The pass's texture changes (the font atlas) come back too: the caller
/// must hand them to the renderer with its first frame (or `clear` them).
pub fn egui_context(limit: u32) -> (egui::Context, egui::TexturesDelta) {
    let ctx = egui::Context::default();
    let output = ctx.run_ui(
        egui::RawInput {
            max_texture_side: Some(limit as usize),
            ..Default::default()
        },
        |_| {},
    );
    (ctx, output.textures_delta)
}

/// Loads `image` into egui, shrunk if it's over the context's limit.
pub fn load_egui(
    ctx: &egui::Context,
    name: &str,
    image: &RgbaImage,
    options: egui::TextureOptions,
) -> egui::TextureHandle {
    let limit = ctx.input(|i| i.max_texture_side) as u32;
    let image = fit(name, image, limit);
    let size = [image.width() as usize, image.height() as usize];
    ctx.load_texture(
        name.to_owned(),
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        options,
    )
}

/// Like `load_egui`, for bytes that are already premultiplied.
pub fn load_egui_premultiplied(
    ctx: &egui::Context,
    name: &str,
    image: &RgbaImage,
    options: egui::TextureOptions,
) -> egui::TextureHandle {
    let limit = ctx.input(|i| i.max_texture_side) as u32;
    let image = fit(name, image, limit);
    let size = [image.width() as usize, image.height() as usize];
    ctx.load_texture(
        name.to_owned(),
        egui::ColorImage::from_rgba_premultiplied(size, image.as_raw()),
        options,
    )
}

// ---- Starting safely -------------------------------------------------------

/// A pack (or packs) that was turned off, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub packs: Vec<String>,
    pub reason: String,
}

impl Dropped {
    /// What the title screen says.
    pub fn message(&self) -> String {
        let (noun, verb) = if self.packs.len() == 1 {
            ("pack", "was")
        } else {
            ("packs", "were")
        };
        format!(
            "Resource {noun} {} failed to load and {verb} turned off: {}",
            self.packs.join(", "),
            self.reason
        )
    }
}

/// Why the extra packs are off before anything is tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// `--safe-mode`: no extra packs this time.
    Safe,
    /// The last start never drew a frame.
    UnfinishedStart,
}

pub struct Started {
    pub look: Look,
    pub gui: GuiTextures,
    /// The message for the title screen, if anything was turned off.
    pub notice: Option<String>,
    /// `options.resource_packs` changed and should be saved.
    pub options_changed: bool,
}

/// Where the "starting" marker is: beside `options.ron`.
pub fn marker_path() -> PathBuf {
    Options::path().with_file_name("starting")
}

/// Notes that a start has begun. Returns true if the last one never
/// finished (its marker was still there).
pub fn begin(marker: &Path) -> bool {
    let left = marker.exists();
    if let Some(dir) = marker.parent() {
        let _ = crate::platform::fs::create_dir_all(dir);
    }
    if let Err(err) = crate::platform::fs::write(marker, b"starting\n") {
        log::warn!("can't write {}: {err}", marker.display());
    }
    left
}

/// The first frame was drawn: the start worked.
pub fn finish(marker: &Path) {
    let _ = crate::platform::fs::remove_file(marker);
}

#[cfg(test)]
pub static FAIL_HOOK: Mutex<Option<String>> = Mutex::new(None);

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "it panicked".to_owned())
}

/// Runs `f`; an error or a panic in it comes back as the reason.
pub fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(format!("{err:#}")),
        Err(payload) => Err(panic_text(payload.as_ref())),
    }
}

/// Builds the look for `ids` from the packs in `dirs` (the packs folder and
/// the built-in one). A panic in a bad pack comes back as the reason (the
/// resource-pack thread uses this too).
pub fn build_look(
    assets: &Path,
    dirs: (&Path, &Path),
    ids: &[String],
    mobs: &MobRegistry,
) -> Result<Look, String> {
    guard(|| {
        #[cfg(test)]
        if let Some(bad) = FAIL_HOOK.lock().ok().and_then(|h| h.clone())
            && ids.contains(&bad)
        {
            panic!("injected failure in {bad}");
        }
        set_pack_label(ids);
        let packs = crate::look::open_packs_in(dirs.0, dirs.1, ids);
        Look::build(assets, packs, mobs).context("building the look")
    })
}

/// Builds the look for `ids` and loads its GUI images into `ctx`; a
/// failure or a panic anywhere in it comes back as the reason.
pub fn try_load(
    assets: &Path,
    dirs: (&Path, &Path),
    ids: &[String],
    mobs: &MobRegistry,
    ctx: &egui::Context,
) -> Result<(Look, GuiTextures), String> {
    let look = build_look(assets, dirs, ids, mobs)?;
    let gui = guard(|| {
        GuiTextures::load(ctx, look.atlas.image(), look.figure(), &look.res)
            .context("loading the GUI images")
    })?;
    Ok((look, gui))
}

/// Packs turned off are saved at once, so the next start is clean too.
fn finish_start(
    options: &mut Options,
    save_to: Option<&Path>,
    loaded: (Look, GuiTextures),
    notices: Vec<String>,
    changed: bool,
) -> Started {
    if let (true, Some(path)) = (changed, save_to) {
        options.store_packs();
        if let Err(err) = options.save(path) {
            log::error!("can't save {}: {err:#}", path.display());
        }
    }
    Started {
        look: loaded.0,
        gui: loaded.1,
        notice: (!notices.is_empty()).then(|| notices.join("\n")),
        options_changed: changed,
    }
}

/// Builds the look and the GUI images from the saved packs, and if that
/// fails turns off the pack that did it (found by loading each on its own),
/// or all the extra packs if it can't tell, or the style's own as a last
/// resort. `options.resource_packs` is changed to match; `Started` says so.
pub fn start(
    assets: &Path,
    dirs: (&Path, &Path),
    options: &mut Options,
    mobs: &MobRegistry,
    ctx: &egui::Context,
    mode: Mode,
    save_to: Option<&Path>,
) -> Result<Started> {
    let builtin = dirs.1;
    let mut notices: Vec<String> = Vec::new();
    let mut changed = false;
    let extras = options.resource_packs.clone();
    if mode != Mode::Normal && !extras.is_empty() {
        options.resource_packs.clear();
        changed = mode == Mode::UnfinishedStart;
        notices.push(match mode {
            Mode::Safe => format!(
                "Safe mode: resource packs {} are off this time.",
                extras.join(", ")
            ),
            _ => format!(
                "The last start didn't finish, so resource packs {} were turned off.",
                extras.join(", ")
            ),
        });
    }
    let stack = crate::pack::style_stack(options.visuals.style, builtin);
    let first = match try_load(assets, dirs, &options.active_packs(builtin), mobs, ctx) {
        Ok(loaded) => return Ok(finish_start(options, save_to, loaded, notices, changed)),
        Err(err) => err,
    };
    log::error!("the resource packs failed to load: {first}");
    // Is it the style's own packs? Then fall back to plain vanilla.
    let extras = options.resource_packs.clone();
    let without_extras = match try_load(assets, dirs, &stack, mobs, ctx) {
        Ok(loaded) => Some(loaded),
        Err(err) => {
            log::error!("the style's packs failed to load: {err}");
            let loaded = try_load(assets, dirs, &[], mobs, ctx)
                .map_err(|err| anyhow!("can't load even vanilla: {err}"))?;
            notices.push(
                Dropped {
                    packs: stack.clone(),
                    reason: err,
                }
                .message(),
            );
            options.resource_packs.clear();
            return Ok(finish_start(options, save_to, loaded, notices, true));
        }
    };
    // The extras are at fault. Which? Load each on its own over the stack.
    let culprits: Vec<(String, String)> = if extras.len() == 1 {
        vec![(extras[0].clone(), first.clone())]
    } else {
        extras
            .iter()
            .filter_map(|pack| {
                let mut ids = vec![pack.clone()];
                ids.extend(stack.iter().cloned());
                try_load(assets, dirs, &ids, mobs, ctx)
                    .err()
                    .map(|reason| (pack.clone(), reason))
            })
            .collect()
    };
    for (pack, reason) in &culprits {
        notices.push(
            Dropped {
                packs: vec![pack.clone()],
                reason: reason.clone(),
            }
            .message(),
        );
    }
    let keep: Vec<String> = extras
        .iter()
        .filter(|p| !culprits.iter().any(|(c, _)| c == *p))
        .cloned()
        .collect();
    if !culprits.is_empty() && !keep.is_empty() {
        options.resource_packs = keep.clone();
        match try_load(assets, dirs, &options.active_packs(builtin), mobs, ctx) {
            Ok(loaded) => return Ok(finish_start(options, save_to, loaded, notices, true)),
            Err(reason) => {
                // Fine apart, not together.
                notices.push(
                    Dropped {
                        packs: keep,
                        reason,
                    }
                    .message(),
                );
            }
        }
    } else if culprits.is_empty() {
        notices.push(
            Dropped {
                packs: extras,
                reason: first,
            }
            .message(),
        );
    }
    options.resource_packs.clear();
    let loaded = without_extras.expect("the style's own packs loaded");
    Ok(finish_start(options, save_to, loaded, notices, true))
}

#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use crate::{
        entity::mobs::MobRegistry, look::Look, options::Options, render::visuals::Style,
        ui::gui::GuiTextures,
    };

    fn gpu_limit() -> Option<u32> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter =
            crate::platform::task::block_on(instance.request_adapter(&Default::default())).ok()?;
        Some(adapter.limits().max_texture_dimension_2d)
    }

    /// Step 0: every texture handed to egui and to wgpu, for each style and
    /// with Patrix 32x on top: name, size, over egui's limit, over the GPU's.
    #[test]
    #[ignore = "a report; run with --ignored --nocapture"]
    fn texture_report() {
        let assets = crate::assets::dir();
        let builtin = crate::pack::builtin_dir();
        let Ok(mobs) = MobRegistry::load(&assets.join("data/mobs.ron")) else {
            return;
        };
        let gpu = gpu_limit();
        println!("GPU max_texture_dimension_2d: {gpu:?}");
        let patrix = crate::pack::find_by_words(&builtin, &["patrix"], &[]);
        println!("Patrix file: {patrix:?}");
        for (label, style, extra) in [
            ("vanilla", Style::Vanilla, None),
            ("barebones", Style::Barebones, None),
            ("realistic", Style::Realistic, None),
            ("barebones + Patrix", Style::Barebones, patrix.clone()),
        ] {
            let mut ids: Vec<String> = extra.into_iter().collect();
            ids.extend(crate::pack::style_stack(style, &builtin));
            let look = match Look::with_packs(&assets, &ids, &mobs) {
                Ok(look) => look,
                Err(err) => {
                    println!("{label}: look failed: {err:#}");
                    continue;
                }
            };
            let (ctx, mut delta) = super::egui_context(gpu.unwrap_or(2048));
            delta.clear();
            let egui_limit = ctx.input(|i| i.max_texture_side);
            println!("== {label}: egui limit {egui_limit}");
            let mut all: Vec<(String, u32, u32, &str)> = vec![
                (
                    "atlas".into(),
                    look.atlas.image().width(),
                    look.atlas.image().height(),
                    "wgpu+egui",
                ),
                (
                    "sky.sun".into(),
                    look.sky.sun.width(),
                    look.sky.sun.height(),
                    "wgpu",
                ),
                (
                    "sky.moon".into(),
                    look.sky.moon.width(),
                    look.sky.moon.height(),
                    "wgpu",
                ),
                (
                    "sky.clouds".into(),
                    look.sky.clouds.width(),
                    look.sky.clouds.height(),
                    "wgpu",
                ),
                (
                    "sky.glint".into(),
                    look.sky.glint.width(),
                    look.sky.glint.height(),
                    "wgpu",
                ),
            ];
            let n = look.mob_textures.len();
            for (i, t) in look.mob_textures.iter().enumerate() {
                let name = match n - i {
                    1 => "skin".to_owned(),
                    2 => "armor sheet".to_owned(),
                    3 => "elytra".to_owned(),
                    _ => format!("mob texture {i}"),
                };
                all.push((name, t.width(), t.height(), "wgpu"));
            }
            // The GUI images, as egui allocates them (the debug build
            // panics at the first one over the limit; catch it).
            let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                GuiTextures::load(&ctx, look.atlas.image(), look.figure(), &look.res).map(|_| ())
            }));
            match &loaded {
                Ok(Ok(())) => println!("GuiTextures::load: ok"),
                Ok(Err(err)) => println!("GuiTextures::load: error {err:#}"),
                Err(_) => println!("GuiTextures::load: PANIC"),
            }
            for (id, meta) in ctx.tex_manager().read().allocated() {
                let _ = id;
                all.push((
                    meta.name.clone(),
                    meta.size[0] as u32,
                    meta.size[1] as u32,
                    "egui",
                ));
            }
            for (name, w, h, to) in all {
                let side = w.max(h);
                let over_egui = to.contains("egui") && side as usize > egui_limit;
                let over_gpu = to.contains("wgpu") && gpu.is_some_and(|g| side > g);
                if over_egui || over_gpu || side >= 1024 {
                    println!(
                        "  {name:<28} {w}x{h} -> {to:<9} over egui: {over_egui}, over GPU: {over_gpu}"
                    );
                }
            }
        }
    }

    fn assets_ready() -> Option<(std::path::PathBuf, MobRegistry)> {
        let assets = crate::assets::dir();
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla");
            return None;
        }
        let mobs = MobRegistry::load(&assets.join("data/mobs.ron")).ok()?;
        Some((assets, mobs))
    }

    /// The tests that inject a failing pack share one hook.
    static HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct Hook<'a>(#[allow(dead_code)] std::sync::MutexGuard<'a, ()>);
    impl Hook<'_> {
        fn set(pack: &str) -> Hook<'static> {
            let guard = HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            *super::FAIL_HOOK.lock().unwrap() = Some(pack.to_owned());
            Hook(guard)
        }
    }
    impl Drop for Hook<'_> {
        fn drop(&mut self) {
            *super::FAIL_HOOK.lock().unwrap() = None;
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, image::Rgba([200, 60, 60, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    const MCMETA: &[u8] = br#"{"pack": {"pack_format": 34, "description": "test"}}"#;

    /// Patrix 32x saved in options.ron: the start works and Patrix is on.
    #[test]
    fn starting_with_patrix_saved_works() {
        let Some((assets, mobs)) = assets_ready() else {
            return;
        };
        let builtin = crate::pack::builtin_dir();
        let Some(patrix) = crate::pack::find_by_words(&builtin, &["patrix"], &[]) else {
            eprintln!("skipping: no Patrix");
            return;
        };
        let dir = crate::world::save::TempDir::new("startup-patrix");
        let path = dir.path().join("options.ron");
        let saved = Options {
            resource_packs: vec![patrix.clone()],
            ..Options::default()
        };
        saved.save(&path).unwrap();
        let mut options = Options::load(&path);
        // Both with egui's own default limit (2048) and the GPU's.
        let (big, mut delta) = super::egui_context(16384);
        delta.clear();
        for ctx in [egui::Context::default(), big] {
            let started = super::start(
                &assets,
                (&crate::pack::packs_dir(), &builtin),
                &mut options,
                &mobs,
                &ctx,
                super::Mode::Normal,
                None,
            )
            .expect("starts");
            assert!(started.notice.is_none(), "{:?}", started.notice);
            assert!(!started.options_changed);
            assert_eq!(options.resource_packs, std::slice::from_ref(&patrix));
            assert!(
                started
                    .look
                    .res
                    .layers()
                    .iter()
                    .any(|p| p.name().contains("Patrix")),
                "Patrix is loaded"
            );
            // The armor sheet is a grid now: no side near 2304.
            let sheet = started.look.armor_sheet();
            assert!(
                sheet.width().max(sheet.height()) <= 1024,
                "{}x{}",
                sheet.width(),
                sheet.height()
            );
            let limit = ctx.input(|i| i.max_texture_side);
            for (_, meta) in ctx.tex_manager().read().allocated() {
                assert!(meta.size[0].max(meta.size[1]) <= limit, "{}", meta.name);
            }
        }
    }

    /// A 4096 x 4096 armor layer and a 20000-pixel-tall GUI image load, shrunk.
    #[test]
    fn oversized_pack_images_are_shrunk_not_fatal() {
        let Some((assets, mobs)) = assets_ready() else {
            return;
        };
        let dir = crate::world::save::TempDir::new("startup-huge");
        let pack = dir.path().join("huge.zip");
        crate::pack::tests::write_zip(
            &pack,
            &[
                ("pack.mcmeta", MCMETA),
                (
                    "assets/minecraft/textures/entity/equipment/humanoid/iron.png",
                    &png(4096, 4096),
                ),
                (
                    "assets/minecraft/textures/gui/container/inventory.png",
                    &png(256, 20000),
                ),
            ],
        );
        let ctx = egui::Context::default();
        let none = dir.path().join("none");
        let (look, _gui) = super::try_load(
            &assets,
            (dir.path(), &none),
            &["huge.zip".to_owned()],
            &mobs,
            &ctx,
        )
        .expect("loads");
        let sheet = look.armor_sheet();
        assert!(sheet.width() >= 4096 * 4, "the big layer made a big sheet");
        let limit = ctx.input(|i| i.max_texture_side);
        let mut armor = 0;
        for (_, meta) in ctx.tex_manager().read().allocated() {
            let side = meta.size[0].max(meta.size[1]);
            assert!(side <= limit, "{} is {side}", meta.name);
            armor += usize::from(meta.name == "armor");
        }
        assert_eq!(armor, 1, "the armor texture is there, within the limit");
        // The helper's arithmetic: whole-number factors, nearest.
        assert_eq!(super::shrink_factor(16384, 10240, 2048), 8);
        assert_eq!(super::shrink_factor(256, 20000, 2048), 10);
        let big = RgbaImage::from_pixel(5000, 100, image::Rgba([1, 2, 3, 255]));
        let small = super::fit("test", &big, 2048);
        assert_eq!((small.width(), small.height()), (1667, 34));
        assert!(matches!(
            super::fit("test", &big, 8192),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    /// A pack whose loading panics is turned off, said so, and saved.
    #[test]
    fn a_pack_that_panics_is_turned_off_and_saved() {
        let Some((assets, mobs)) = assets_ready() else {
            return;
        };
        let _hook = Hook::set("bad.zip");
        let dir = crate::world::save::TempDir::new("startup-bad");
        let none = dir.path().join("none");
        let path = dir.path().join("options.ron");
        let mut options = Options {
            resource_packs: vec!["good.zip".to_owned(), "bad.zip".to_owned()],
            ..Options::default()
        };
        options.save(&path).unwrap();
        let ctx = egui::Context::default();
        let started = super::start(
            &assets,
            (dir.path(), &none),
            &mut options,
            &mobs,
            &ctx,
            super::Mode::Normal,
            Some(&path),
        )
        .expect("starts without the bad pack");
        assert_eq!(
            started.notice.as_deref(),
            Some(
                "Resource pack bad.zip failed to load and was turned off: injected failure in bad.zip"
            )
        );
        assert_eq!(options.resource_packs, ["good.zip"]);
        assert_eq!(
            Options::load(&path).resource_packs,
            ["good.zip"],
            "options.ron updated"
        );
        // Only a bad pack: all the extras go.
        options.resource_packs = vec!["bad.zip".to_owned()];
        let started = super::start(
            &assets,
            (dir.path(), &none),
            &mut options,
            &mobs,
            &ctx,
            super::Mode::Normal,
            Some(&path),
        )
        .expect("starts");
        assert!(started.options_changed && options.resource_packs.is_empty());
        assert!(Options::load(&path).resource_packs.is_empty());
    }

    /// A leftover marker, or --safe-mode, starts without extra packs.
    #[test]
    fn a_leftover_marker_or_safe_mode_skips_extra_packs() {
        let Some((assets, mobs)) = assets_ready() else {
            return;
        };
        let dir = crate::world::save::TempDir::new("startup-marker");
        let marker = dir.path().join("starting");
        assert!(!super::begin(&marker), "the first start finds no marker");
        assert!(
            super::begin(&marker),
            "the marker is still there: it never finished"
        );
        super::finish(&marker);
        assert!(!marker.exists());
        assert!(!super::begin(&marker), "a finished start leaves none");
        let none = dir.path().join("none");
        let path = dir.path().join("options.ron");
        let ctx = egui::Context::default();
        for (mode, saved, words) in [
            (super::Mode::UnfinishedStart, true, "didn't finish"),
            (super::Mode::Safe, false, "Safe mode"),
        ] {
            let mut options = Options {
                resource_packs: vec!["extra.zip".to_owned()],
                ..Options::default()
            };
            options.save(&path).unwrap();
            let started = super::start(
                &assets,
                (dir.path(), &none),
                &mut options,
                &mobs,
                &ctx,
                mode,
                Some(&path),
            )
            .expect("starts");
            assert!(options.resource_packs.is_empty());
            assert!(started.notice.unwrap().contains(words));
            assert_eq!(Options::load(&path).resource_packs.is_empty(), saved);
        }
    }

    /// Choosing a pack that fails while the game runs keeps the old look
    /// and doesn't save the choice.
    #[test]
    fn a_pack_that_fails_while_running_is_not_kept_or_saved() {
        let Some((_assets, mobs)) = assets_ready() else {
            return;
        };
        let _hook = Hook::set("bad.zip");
        let mut app = crate::app::App::new(None);
        app.set_mob_defs_for_test(std::sync::Arc::new(mobs));
        let good = app
            .options_for_test()
            .active_packs(&crate::pack::builtin_dir());
        app.options_for_test().resource_packs = vec!["bad.zip".to_owned()];
        app.apply_resource_packs_for_test();
        let started = crate::platform::time::Instant::now();
        while app.poll_pending_for_test() {
            assert!(started.elapsed().as_secs() < 60, "never finished");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            app.options_for_test().resource_packs.is_empty(),
            "the choice is undone"
        );
        assert_eq!(app.applied_packs_for_test(), good, "the old look stays");
        // The test options file is per process; the bad pack isn't in it.
        let on_disk = crate::platform::fs::read_to_string(Options::path()).unwrap_or_default();
        assert!(!on_disk.contains("bad.zip"), "not saved: {on_disk}");
    }
}

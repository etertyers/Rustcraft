//! The browser entry (M71). JavaScript (`web/loader.js`) calls these exports
//! in order: `boot` (asset storage, persisted files), then, if the vanilla
//! layer is missing, the setup screen's `vanilla_add*` / `vanilla_finish` and
//! `pack_add`, then `start`.
//!
//! The browser has no synchronous files, so the game's `platform::fs` reads
//! from memory: `boot` unpacks the repo's assets (one fetched archive) and
//! whatever the player stored in OPFS before the game starts, and writes of
//! persisted files (options, packs) go to OPFS in the background.

mod store;
pub mod workers;

use std::io::{Cursor, Read};

use wasm_bindgen::prelude::*;

use crate::platform::{fs, layers};

#[wasm_bindgen(module = "/src/web/glue.js")]
extern "C" {
    #[wasm_bindgen(js_name = opfsList, catch)]
    pub async fn opfs_list() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = opfsIdle, catch)]
    async fn opfs_idle() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = opfsRead, catch)]
    pub async fn opfs_read(path: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = opfsWrite, catch)]
    pub async fn opfs_write(path: &str, bytes: &[u8]) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = opfsDelete, catch)]
    pub async fn opfs_delete(path: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = queryString)]
    pub fn query_string() -> String;
    #[wasm_bindgen(js_name = setStatus)]
    pub fn set_status(text: &str);
    #[wasm_bindgen(js_name = hideOverlay)]
    pub fn hide_overlay();
    #[wasm_bindgen(js_name = storageEstimate, catch)]
    pub async fn storage_estimate() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = onPointerLockLost)]
    pub fn on_pointer_lock_lost(cb: &Closure<dyn FnMut()>);
    #[wasm_bindgen(js_name = onLeavingPage)]
    fn on_leaving_page(cb: &Closure<dyn FnMut()>);
    #[wasm_bindgen(js_name = installPointerLock)]
    pub fn install_pointer_lock(canvas: &web_sys::HtmlCanvasElement);
    #[wasm_bindgen(js_name = installKeyboardLock)]
    pub fn install_keyboard_lock();
    #[wasm_bindgen(js_name = listenForGesture)]
    pub fn listen_for_gesture();
    #[wasm_bindgen(js_name = hadGesture)]
    pub fn had_gesture() -> bool;
    #[wasm_bindgen(js_name = downloadBytes)]
    fn download_bytes_js(filename: &str, bytes: &[u8]);
    #[wasm_bindgen(js_name = pickFile)]
    fn pick_file_js(accept: &str, cb: &Closure<dyn FnMut(js_sys::Uint8Array)>);
    #[wasm_bindgen(js_name = hardwareConcurrency)]
    pub fn hardware_concurrency() -> u32;
}

/// Saves `bytes` as a download.
pub fn download_bytes(filename: &str, bytes: &[u8]) {
    download_bytes_js(filename, bytes);
}

/// Asks for a file; it arrives through `platform::window::take_picked_zip`.
pub fn pick_file(accept: &str) {
    let cb = Closure::<dyn FnMut(js_sys::Uint8Array)>::new(|bytes: js_sys::Uint8Array| {
        crate::platform::window::set_picked(bytes.to_vec());
    });
    pick_file_js(accept, &cb);
    cb.forget();
}

const VANILLA_ZIP: &str = "vanilla.zip";
const VANILLA_ROOT: &str = "assets/vanilla/assets/minecraft";

fn js_err(err: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&err.to_string())
}

/// Writes `data` at `path` in memory, making the folders.
fn put(path: &str, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data)
}

/// Unpacks a zip into `dest` (a prefix such as `assets/`), keeping each
/// entry's path under it. Returns the number of files.
fn unzip_into(
    bytes: &[u8],
    dest: &str,
    keep: impl Fn(&str) -> Option<String>,
) -> Result<u32, JsValue> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(js_err)?;
    let mut count = 0;
    let mut buf = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(js_err)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map(|n| n.into_owned()).map_err(js_err)?;
        let Some(rel) = keep(&name) else {
            continue;
        };
        buf.clear();
        entry.read_to_end(&mut buf).map_err(js_err)?;
        put(&format!("{dest}{rel}"), &buf).map_err(js_err)?;
        count += 1;
    }
    Ok(count)
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response =
        wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
            .await?
            .dyn_into()?;
    if !response.ok() {
        return Err(js_err(format!("{url}: HTTP {}", response.status())));
    }
    let buf = wasm_bindgen_futures::JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// Step 1. Installs the storage, unpacks the repo's assets (`assets_url`, the
/// archive `scripts/build-web.sh` made) and what OPFS holds. Returns whether
/// the vanilla layer is present.
#[wasm_bindgen]
pub async fn boot(assets_url: String) -> Result<bool, JsValue> {
    console_error_panic_hook::set_once();
    crate::platform::logging::init();
    store::install();
    set_status("Loading the game's own assets...");
    let archive = fetch_bytes(&assets_url).await?;
    let n = unzip_into(&archive, "assets/", |name| Some(name.to_owned()))?;
    log::info!("{n} asset files unpacked");
    set_status("Restoring your settings and packs...");
    let mut vanilla = false;
    let listed = opfs_list().await?;
    log::info!("OPFS holds {} files", js_sys::Array::from(&listed).length());
    for path in js_sys::Array::from(&listed)
        .iter()
        .filter_map(|v| v.as_string())
    {
        let Some(data) = opfs_read(&path).await.ok().filter(|v| !v.is_undefined()) else {
            continue;
        };
        let bytes = js_sys::Uint8Array::new(&data).to_vec();
        if path == VANILLA_ZIP {
            set_status("Unpacking Minecraft's files...");
            let n = unzip_into(&bytes, "", layers::vanilla_destination)?;
            log::info!("vanilla layer: {n} files from OPFS");
            vanilla = true;
        } else {
            log::info!("restored {path} ({} bytes)", bytes.len());
            put(&path, &bytes).map_err(js_err)?;
        }
    }
    let report = vanilla_report();
    log::info!("{}", report.summary());
    Ok(vanilla && report.complete())
}

fn vanilla_report() -> layers::VanillaReport {
    let prefix = format!("{VANILLA_ROOT}/");
    let paths = store::paths_under(&prefix);
    layers::check_vanilla(&paths, &|p| fs::read(format!("{prefix}{p}")).ok())
}

/// Setup screen: one picked file (a folder pick gives each file's relative
/// path). Returns whether it belongs to the vanilla layer.
#[wasm_bindgen]
pub fn vanilla_add(path: &str, bytes: &[u8]) -> bool {
    match layers::vanilla_destination(path) {
        Some(dest) => put(&dest, bytes).is_ok(),
        None => false,
    }
}

/// Setup screen: a picked .zip (a jar or a download of the assets).
#[wasm_bindgen]
pub fn vanilla_add_zip(bytes: &[u8]) -> Result<u32, JsValue> {
    unzip_into(bytes, "", layers::vanilla_destination)
}

/// Setup screen: checks what was added and, if complete, keeps it in OPFS.
/// Returns the summary; throws the problems.
#[wasm_bindgen]
pub async fn vanilla_finish() -> Result<String, JsValue> {
    let report = vanilla_report();
    if !report.complete() {
        return Err(js_err(report.summary()));
    }
    set_status("Saving Minecraft's files in the browser...");
    let zip = store::pack_zip(&format!("{VANILLA_ROOT}/"), "assets/minecraft/").map_err(js_err)?;
    opfs_write(VANILLA_ZIP, &zip).await?;
    Ok(report.summary())
}

/// Setup screen or Resource Packs: a resource pack, kept as a built-in one
/// (`assets/downloads/resourcepacks/`) like the Mac app's.
#[wasm_bindgen]
pub fn pack_add(name: &str, bytes: &[u8]) -> Result<(), JsValue> {
    let name = name.rsplit('/').next().unwrap_or(name);
    put(&format!("assets/downloads/resourcepacks/{name}"), bytes).map_err(js_err)
}

/// A saved world as a .zip (the Select World screen's Export; also for
/// scripts).
#[wasm_bindgen]
pub fn export_world(name: &str) -> Result<Vec<u8>, JsValue> {
    crate::platform::world_zip::export(&crate::level::saves_dir(), name).map_err(js_err)
}

/// Unpacks a world .zip into the saves; returns the folder name it got.
#[wasm_bindgen]
pub fn import_world(bytes: &[u8]) -> Result<String, JsValue> {
    crate::platform::world_zip::import(&crate::level::saves_dir(), bytes).map_err(js_err)
}

/// A file from the browser's storage (the perf test reads its results).
#[wasm_bindgen]
pub fn read_file(path: &str) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

/// Resolves when everything written so far has reached OPFS.
#[wasm_bindgen]
pub async fn storage_idle() -> Result<(), JsValue> {
    opfs_idle().await.map(|_| ())
}

/// Saves the world now (the page's tab-hidden and unload hooks call it; the
/// tests do too).
#[wasm_bindgen]
pub fn save_now() {
    crate::platform::window::request_save();
}

/// Step 3: the game. `?creative`, `?world=Name`, `?seed=5`, `?distance=9`,
/// `?perf-test`, ... are the native flags (`--creative`, ...).
#[wasm_bindgen]
pub fn start() -> Result<(), JsValue> {
    use winit::platform::web::EventLoopExtWebSys;
    let args = crate::platform::env::query_args(&query_string());
    crate::platform::env::set_args(args.clone());
    let cli = crate::cli::parse(args.iter().skip(1).cloned()).map_err(js_err)?;
    let mut app = crate::app::App::new(cli.world)
        .with_bench(cli.bench)
        .with_view(cli.view)
        .with_size(cli.size)
        .with_graphics(cli.style, cli.shaders, cli.quality);
    if let Some((shot, out)) = cli.shot {
        app = app.with_shot(shot, out);
    }
    if let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("rustcraft"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
    {
        install_pointer_lock(&canvas);
    }
    install_keyboard_lock();
    listen_for_gesture();
    let lost = Closure::<dyn FnMut()>::new(crate::platform::window::set_lock_lost);
    on_pointer_lock_lost(&lost);
    lost.forget();
    let leaving = Closure::<dyn FnMut()>::new(crate::platform::window::request_save);
    on_leaving_page(&leaving);
    leaving.forget();
    hide_overlay();
    let event_loop = winit::event_loop::EventLoop::new().map_err(js_err)?;
    event_loop.spawn_app(app);
    Ok(())
}

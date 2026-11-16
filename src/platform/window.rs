//! The window and the GPU handshake (M71). Native: a winit window, and the
//! GPU is asked for (and waited on) inside `Renderer::new`. Browser: the
//! window is a `<canvas id="rustcraft">` from the page, and the GPU request
//! is `async`, so `acquire` starts it and says `Pending` until it has
//! finished; the app tries again on the next event.

use std::sync::Arc;

use winit::window::{Window, WindowAttributes};

use crate::render::Gpu;

/// What `acquire` found.
pub enum Acquire {
    /// Native: `Renderer::new` asks for the GPU itself (`None`). Browser: the
    /// GPU that arrived.
    Ready(Option<Gpu>),
    /// Not yet; try again on the next event.
    Pending,
    /// The browser refused (no WebGPU or WebGL2).
    Failed(String),
}

/// Puts the page's canvas into the window's attributes in the browser.
#[cfg(not(target_arch = "wasm32"))]
pub fn attributes(attrs: WindowAttributes) -> WindowAttributes {
    attrs
}

#[cfg(target_arch = "wasm32")]
pub fn attributes(attrs: WindowAttributes) -> WindowAttributes {
    use wasm_bindgen::JsCast;
    use winit::platform::web::WindowAttributesExtWebSys;
    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("rustcraft"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
    attrs.with_canvas(canvas).with_prevent_default(true)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn acquire(_window: &Arc<Window>) -> Acquire {
    Acquire::Ready(None)
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static SLOT: std::cell::RefCell<Slot> = const { std::cell::RefCell::new(Slot::Idle) };
}

#[cfg(target_arch = "wasm32")]
enum Slot {
    Idle,
    Asked,
    Done(Result<Gpu, String>),
}

#[cfg(target_arch = "wasm32")]
pub fn acquire(window: &Arc<Window>) -> Acquire {
    SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        match std::mem::replace(&mut *slot, Slot::Asked) {
            Slot::Idle => {
                let window = window.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let result = Gpu::request(window.clone())
                        .await
                        .map_err(|err| format!("{err:#}"));
                    SLOT.with(|s| *s.borrow_mut() = Slot::Done(result));
                    window.request_redraw();
                });
                Acquire::Pending
            }
            Slot::Asked => Acquire::Pending,
            Slot::Done(Ok(gpu)) => Acquire::Ready(Some(gpu)),
            Slot::Done(Err(err)) => {
                *slot = Slot::Done(Err(err.clone()));
                Acquire::Failed(err)
            }
        }
    })
}

/// Whether the player has clicked or pressed a key yet. Browsers keep audio
/// (and the pointer lock) off until then; native has no such rule.
#[cfg(not(target_arch = "wasm32"))]
pub fn user_gesture() -> bool {
    true
}

#[cfg(target_arch = "wasm32")]
pub fn user_gesture() -> bool {
    crate::web::had_gesture()
}

static LOCK_LOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The browser ended the pointer lock (Esc, a tab switch): the game acts as
/// if the window lost focus and opens the pause menu, as Minecraft does.
pub fn set_lock_lost() {
    LOCK_LOST.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// True once per loss of the pointer lock.
pub fn take_lock_lost() -> bool {
    LOCK_LOST.swap(false, std::sync::atomic::Ordering::Relaxed)
}

static PICKED: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

/// A file the player picked (see `pick_zip`) arrived.
pub fn set_picked(bytes: Vec<u8>) {
    *PICKED.lock().unwrap_or_else(|e| e.into_inner()) = Some(bytes);
}

/// The file picked since the last call, if any.
pub fn take_picked_zip() -> Option<Vec<u8>> {
    PICKED.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Browser: gives the player `bytes` as a download named `filename`.
#[cfg(target_arch = "wasm32")]
pub fn download(filename: &str, bytes: &[u8]) {
    crate::web::download_bytes(filename, bytes);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn download(filename: &str, _bytes: &[u8]) {
    log::warn!("download {filename}: only the browser build has downloads");
}

/// Browser: asks the player for a .zip; `take_picked_zip` gets it later.
#[cfg(target_arch = "wasm32")]
pub fn pick_zip() {
    crate::web::pick_file(".zip");
}

#[cfg(not(target_arch = "wasm32"))]
pub fn pick_zip() {
    log::warn!("pick_zip: only the browser build has a file picker");
}

static SAVE_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Browser: the tab was hidden or is closing, so the world should be saved
/// now (a page can't count on a later chance).
pub fn request_save() {
    SAVE_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// True once per `request_save`.
pub fn take_save_request() -> bool {
    SAVE_REQUESTED.swap(false, std::sync::atomic::Ordering::Relaxed)
}

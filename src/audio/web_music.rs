//! Background music in the browser (M76): a media element streams and
//! decodes the Ogg file off the page's thread, where kira (no streaming on
//! `wasm32`) would decode the whole track on it: a stall of about 450 ms and
//! 390 MiB at the start of every track. Used where the browser plays Ogg
//! Vorbis; otherwise kira does it, as before.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/web/glue.js")]
extern "C" {
    #[wasm_bindgen(js_name = musicSupported)]
    fn supported_js() -> bool;
    #[wasm_bindgen(js_name = musicStart)]
    fn start_js(bytes: &[u8], volume: f32) -> u32;
    #[wasm_bindgen(js_name = musicPlaying)]
    fn playing_js(id: u32) -> bool;
    #[wasm_bindgen(js_name = musicVolume)]
    fn volume_js(id: u32, volume: f32);
    #[wasm_bindgen(js_name = musicFade)]
    fn fade_js(id: u32, seconds: f32);
}

/// A track playing in a media element.
pub struct Element(u32);

/// Whether this browser plays Ogg Vorbis in a media element.
pub fn supported() -> bool {
    thread_local! {
        static SUPPORTED: bool = supported_js();
    }
    SUPPORTED.with(|s| *s)
}

/// Starts the Ogg file `bytes` at `volume` (0 to 1).
pub fn start(bytes: &[u8], volume: f32) -> Option<Element> {
    supported().then(|| Element(start_js(bytes, volume)))
}

impl Element {
    pub fn playing(&self) -> bool {
        playing_js(self.0)
    }

    pub fn set_volume(&self, volume: f32) {
        volume_js(self.0, volume);
    }

    pub fn fade_out(&self, seconds: f32) {
        fade_js(self.0, seconds);
    }
}

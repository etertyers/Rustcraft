//! The server in its own Web Worker (M76). `web/worker.js` starts another
//! instance of this wasm, hands it one end of a `MessageChannel` and a
//! [`WorldSpec`]; [`server_worker_run`] then builds the world's `Game`
//! and `Server` there, ticks it from a timer at 20 per second, and talks to
//! the page's client through `WorkerTransport` over the port. The page never
//! waits for the server and the server never waits for a frame.
//!
//! The server needs no textures: lighting and shapes only need the block
//! models with placeholder texture rects (as `ChunkManager`'s tests build
//! them), the repo's own `data/` and the recipes, so it starts from
//! `assets.zip` alone, never the Minecraft files or packs.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use wasm_bindgen::prelude::*;
use web_sys::MessagePort;

use crate::{
    game::Game,
    inventory::{crafting::Recipes, stack::Items},
    level::WorldDir,
    net::{
        protocol::{ClientMsg, ServerMsg},
        worker::WorkerTransport,
    },
    platform::{server_worker::WorldSpec, time::Instant},
    server::Server,
    world::{biome::BiomeColors, mesh::BlockModels},
};

use super::port::MessagePortPort;

#[wasm_bindgen(module = "/src/web/glue.js")]
extern "C" {
    #[wasm_bindgen(js_name = everyMs)]
    fn every_ms(ms: u32, callback: &Closure<dyn FnMut()>) -> JsValue;
    #[wasm_bindgen(js_name = workerPost)]
    fn worker_post(message: &JsValue);
}

/// The server's inputs, built from `assets.zip`'s data alone.
fn build_game(spec: &WorldSpec) -> anyhow::Result<Game> {
    let assets = crate::assets::dir();
    let recipes = Recipes::load(&assets.join("data/recipes.ron"))?;
    let items = Items::new().with_recipes(Arc::new(recipes));
    let mobs = Arc::new(crate::entity::mobs::MobRegistry::load(
        &assets.join("data/mobs.ron"),
    )?);
    let models = Arc::new(BlockModels::new(
        crate::world::block::registry(),
        |_| Some([0.0; 4]),
        [0.0; 4],
        BiomeColors::uniform(),
    ));
    let saves = crate::level::saves_dir();
    let dir = WorldDir::new(saves.join(&spec.name));
    let game = if dir.exists() {
        Game::open(dir, items, models)?
    } else {
        let seed = spec.seed.unwrap_or_else(crate::level::random_seed);
        Game::create(&saves, &spec.name, seed, spec.mode, items, models)?
    };
    let mut game = game.with_mobs(mobs);
    game.cheats |= spec.cheats;
    Ok(game)
}

thread_local! {
    static SERVER: RefCell<Option<Server>> = const { RefCell::new(None) };
}

/// Worker side: saves the world now.
#[wasm_bindgen]
pub fn server_worker_save() {
    SERVER.with(|s| {
        if let Some(server) = s.borrow_mut().as_mut()
            && let Err(err) = server.game.save()
        {
            log::error!("server worker: saving failed: {err:#}");
        }
    });
}

/// Worker side: tells guests the world is closing and saves it.
#[wasm_bindgen]
pub fn server_worker_close() {
    SERVER.with(|s| {
        if let Some(server) = s.borrow_mut().as_mut() {
            server.shut_down();
        }
    });
    server_worker_save();
}

/// Worker side: the whole server. Resolves once it's running; the timer keeps
/// it alive after that.
#[wasm_bindgen]
pub async fn server_worker_run(
    port: MessagePort,
    assets_url: String,
    spec: Vec<u8>,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    crate::platform::logging::init();
    let _ = crate::platform::storage::set_storage(Box::new(
        crate::platform::storage::MemoryStorage::new(),
    ));
    let spec = WorldSpec::from_bytes(&spec).ok_or("unreadable world spec")?;
    let archive = super::fetch_bytes(&assets_url).await?;
    let n = super::unzip_into(&archive, "assets/", |name| Some(name.to_owned()))?;
    log::info!("server worker: {n} asset files unpacked");
    let game = build_game(&spec).map_err(super::js_err)?;
    let mut server = Server::new(game);
    let transport: WorkerTransport<ServerMsg, ClientMsg, MessagePortPort> =
        WorkerTransport::new(MessagePortPort::new(port));
    server.connect(Box::new(transport));
    log::info!("server worker: world {:?} running", spec.name);
    SERVER.with(|s| *s.borrow_mut() = Some(server));
    let last = Rc::new(RefCell::new(Instant::now()));
    let tick = Closure::<dyn FnMut()>::new(move || {
        let now = Instant::now();
        let dt = now.duration_since(*last.borrow()).as_secs_f32();
        *last.borrow_mut() = now;
        SERVER.with(|s| {
            let mut s = s.borrow_mut();
            let Some(server) = s.as_mut() else { return };
            server.advance(dt.min(0.25));
            let status = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&status, &"type".into(), &"status".into());
            let _ = js_sys::Reflect::set(&status, &"cheats".into(), &server.game.cheats.into());
            let _ = js_sys::Reflect::set(&status, &"stopping".into(), &server.stopping.into());
            worker_post(&status);
        });
    });
    let _timer = every_ms(10, &tick);
    tick.forget();
    Ok(())
}

// The page's side: starting the worker and what it reports back.

#[derive(Default)]
struct Status {
    cheats: bool,
    stopping: bool,
}

/// The page's handle on its server worker.
pub struct Remote {
    worker: web_sys::Worker,
    status: Rc<RefCell<Status>>,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

impl Remote {
    pub fn cheats(&self) -> bool {
        self.status.borrow().cheats
    }

    pub fn stopping(&self) -> bool {
        self.status.borrow().stopping
    }

    fn command(&self, what: &str) {
        let message = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&message, &"type".into(), &what.into());
        let _ = self.worker.post_message(&message);
    }

    /// Asks the worker to save the world.
    pub fn save(&self) {
        self.command("save");
    }

    /// Asks the worker to save the world and close it.
    pub fn close(&self) {
        self.command("close");
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        // Give a close request a moment to be handled before the worker goes.
        self.worker.terminate();
    }
}

/// Whether `?server-worker` is in the URL (until saves go through the worker,
/// the page's own saves are what the world list shows).
pub fn requested() -> bool {
    super::query_string().contains("server-worker")
}

/// Starts a server worker playing `spec`; returns the client's end of the
/// connection and the handle on the worker.
pub fn spawn(spec: &WorldSpec) -> anyhow::Result<(crate::client::ClientConnection, Remote)> {
    let fail = |err: JsValue| anyhow::anyhow!("{err:?}");
    let options = web_sys::WorkerOptions::new();
    options.set_type(web_sys::WorkerType::Module);
    let worker = web_sys::Worker::new_with_options("worker.js", &options).map_err(fail)?;
    let channel = web_sys::MessageChannel::new().map_err(fail)?;
    let status = Rc::new(RefCell::new(Status::default()));
    let state = status.clone();
    let on_message =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let data = event.data();
            let get = |key: &str| {
                js_sys::Reflect::get(&data, &key.into())
                    .ok()
                    .and_then(|v| v.as_bool())
            };
            if js_sys::Reflect::get(&data, &"type".into())
                .ok()
                .and_then(|v| v.as_string())
                .as_deref()
                == Some("status")
            {
                let mut s = state.borrow_mut();
                s.cheats = get("cheats").unwrap_or(false);
                s.stopping = get("stopping").unwrap_or(false);
            }
        });
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    let init = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&init, &"type".into(), &"init".into());
    let _ = js_sys::Reflect::set(&init, &"kind".into(), &"server".into());
    worker.post_message(&init).map_err(fail)?;
    let start = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&start, &"type".into(), &"server".into());
    let _ = js_sys::Reflect::set(&start, &"port".into(), &channel.port2());
    let _ = js_sys::Reflect::set(&start, &"assetsUrl".into(), &"assets.zip".into());
    let bytes = js_sys::Uint8Array::from(spec.to_bytes().as_slice());
    let _ = js_sys::Reflect::set(&start, &"spec".into(), &bytes);
    let transfer = js_sys::Array::of1(&channel.port2());
    worker
        .post_message_with_transfer(&start, &transfer)
        .map_err(fail)?;
    let conn: WorkerTransport<ClientMsg, ServerMsg, MessagePortPort> =
        WorkerTransport::new(MessagePortPort::new(channel.port1()));
    Ok((
        Box::new(conn),
        Remote {
            worker,
            status,
            _on_message: on_message,
        },
    ))
}

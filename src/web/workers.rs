//! Web Workers (M72). Each worker is another instance of this crate's wasm
//! (its own memory; nothing is shared, so no special headers are needed),
//! started from `worker.js`, which loads the same `pkg/rustcraft.js`.
//!
//! Generator workers: a request (`world::offload::GenRequest`) goes in as a
//! transferred `ArrayBuffer`, the chunk's save encoding comes back the same
//! way. `JsGenerators` is the page-side pool; every `ChunkManager` that
//! generates shares it through a [`Handle`].

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{MessageEvent, Worker, WorkerOptions, WorkerType};

use crate::world::{
    chunk::ChunkPos,
    offload::{GenPool, GenRequest, GeneratorWorker},
};

/// How many generator workers: `min(3, hardwareConcurrency - 2)`, at least 1
/// (Kenzo's rule; the main thread and the server keep their own cores).
pub fn generator_count(cores: u32) -> usize {
    (cores.saturating_sub(2) as usize).clamp(1, 3)
}

/// How many requests each generator worker may hold at once: enough to keep
/// it busy while the answer to the last one travels.
const PER_WORKER: usize = 2;

#[derive(Default)]
struct Shared {
    /// (handle, position, bytes)
    finished: Vec<(u32, ChunkPos, Vec<u8>)>,
    /// Request id to (handle, position).
    asked: HashMap<u32, (u32, ChunkPos)>,
    /// Requests each worker holds.
    load: Vec<usize>,
    next_id: u32,
    next_handle: u32,
}

/// The page's generator workers.
pub struct JsGenerators {
    workers: Vec<Worker>,
    shared: Rc<RefCell<Shared>>,
    _listeners: Vec<Closure<dyn FnMut(MessageEvent)>>,
}

thread_local! {
    static POOL: RefCell<Option<Rc<RefCell<JsGenerators>>>> = const { RefCell::new(None) };
}

impl JsGenerators {
    fn start(count: usize) -> Result<Self, JsValue> {
        let shared = Rc::new(RefCell::new(Shared {
            load: vec![0; count],
            ..Shared::default()
        }));
        let (mut workers, mut listeners) = (Vec::new(), Vec::new());
        for index in 0..count {
            let options = WorkerOptions::new();
            options.set_type(WorkerType::Module);
            let worker = Worker::new_with_options("worker.js", &options)?;
            let state = shared.clone();
            let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
                let data = event.data();
                let id = js_sys::Reflect::get(&data, &"id".into())
                    .ok()
                    .and_then(|v| v.as_f64())
                    .map(|v| v as u32);
                let bytes = js_sys::Reflect::get(&data, &"bytes".into())
                    .ok()
                    .map(|v| js_sys::Uint8Array::new(&v).to_vec());
                let (Some(id), Some(bytes)) = (id, bytes) else {
                    return; // the "ready" message
                };
                let mut shared = state.borrow_mut();
                shared.load[index] = shared.load[index].saturating_sub(1);
                if let Some((handle, pos)) = shared.asked.remove(&id) {
                    shared.finished.push((handle, pos, bytes));
                }
            });
            worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            let init = js_sys::Object::new();
            js_sys::Reflect::set(&init, &"type".into(), &"init".into())?;
            js_sys::Reflect::set(&init, &"kind".into(), &"generator".into())?;
            worker.post_message(&init)?;
            workers.push(worker);
            listeners.push(on_message);
        }
        Ok(Self {
            workers,
            shared,
            _listeners: listeners,
        })
    }

    fn submit(&mut self, handle: u32, req: GenRequest) {
        let mut shared = self.shared.borrow_mut();
        let Some(worker) = (0..self.workers.len()).min_by_key(|&i| shared.load[i]) else {
            return;
        };
        let id = shared.next_id;
        shared.next_id = shared.next_id.wrapping_add(1);
        shared.asked.insert(id, (handle, req.pos));
        shared.load[worker] += 1;
        let bytes = js_sys::Uint8Array::from(req.to_bytes().as_slice());
        let message = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&message, &"id".into(), &id.into());
        let _ = js_sys::Reflect::set(&message, &"bytes".into(), &bytes);
        let transfer = js_sys::Array::of1(&bytes.buffer());
        let _ = self.workers[worker].post_message_with_transfer(&message, &transfer);
    }
}

/// One `ChunkManager`'s view of the shared pool.
pub struct Handle {
    id: u32,
    pool: Rc<RefCell<JsGenerators>>,
}

// SAFETY: the browser build runs the game on one thread (the page's main
// thread, or a worker for the server); a `Handle` never leaves it.
unsafe impl Send for Handle {}

impl GenPool for Handle {
    fn submit(&mut self, req: GenRequest) {
        self.pool.borrow_mut().submit(self.id, req);
    }

    fn poll(&mut self, out: &mut Vec<(ChunkPos, Vec<u8>)>) {
        let pool = self.pool.borrow();
        let mut shared = pool.shared.borrow_mut();
        let mut i = 0;
        while i < shared.finished.len() {
            if shared.finished[i].0 == self.id {
                let (_, pos, bytes) = shared.finished.swap_remove(i);
                out.push((pos, bytes));
            } else {
                i += 1;
            }
        }
    }

    fn capacity(&self) -> usize {
        self.pool.borrow().workers.len() * PER_WORKER
    }
}

/// A handle on the page's generator workers, starting them the first time;
/// `None` when workers can't start or `?single-thread` is in the URL.
pub fn generators() -> Option<Box<dyn GenPool>> {
    if super::query_string().contains("single-thread") {
        return None;
    }
    POOL.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            let count = generator_count(super::hardware_concurrency());
            match JsGenerators::start(count) {
                Ok(pool) => {
                    log::info!("{count} generator workers started");
                    *cell = Some(Rc::new(RefCell::new(pool)));
                }
                Err(err) => {
                    log::warn!("no generator workers ({err:?}); generating on the main thread");
                    return None;
                }
            }
        }
        let pool = cell.clone()?;
        let id = {
            let p = pool.borrow();
            let mut shared = p.shared.borrow_mut();
            shared.next_handle += 1;
            shared.next_handle
        };
        Some(Box::new(Handle { id, pool }) as Box<dyn GenPool>)
    })
}

thread_local! {
    static GENERATOR: RefCell<GeneratorWorker> = RefCell::new(GeneratorWorker::default());
}

/// Worker side: one generation request in, the chunk's bytes out.
#[wasm_bindgen]
pub fn worker_generate(request: &[u8]) -> Option<Vec<u8>> {
    GENERATOR.with(|g| g.borrow_mut().handle(request))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_counts_follow_the_rule() {
        assert_eq!(generator_count(1), 1);
        assert_eq!(generator_count(2), 1);
        assert_eq!(generator_count(4), 2);
        assert_eq!(generator_count(5), 3);
        assert_eq!(generator_count(16), 3);
    }
}

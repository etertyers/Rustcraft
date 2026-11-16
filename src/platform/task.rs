//! Background work (M70). Native: `std::thread`. Browser: workers, which M72
//! provides; until then [`spawn`] reports that it can't, and game code that
//! needs a thread falls back to doing the work on the calling thread
//! (single thread mode).

use std::io;

/// Whether `spawn` can start a thread here.
pub const THREADS: bool = cfg!(not(target_arch = "wasm32"));

#[cfg(not(target_arch = "wasm32"))]
pub use std::thread::JoinHandle;

/// What `spawn` returns; in the browser it never exists (`spawn` fails), so
/// this only gives the type the same methods.
#[cfg(target_arch = "wasm32")]
pub struct JoinHandle<T>(std::marker::PhantomData<T>);

#[cfg(target_arch = "wasm32")]
impl<T> JoinHandle<T> {
    pub fn join(self) -> std::thread::Result<T> {
        unreachable!("no threads in the browser build")
    }
    pub fn is_finished(&self) -> bool {
        true
    }
}

/// Starts a named thread; `Err` where threads don't exist. For a long-lived
/// worker that loops on a channel: the caller falls back to single thread
/// mode when this fails.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn<T, F>(name: impl Into<String>, f: F) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::Builder::new().name(name.into()).spawn(f)
}

#[cfg(target_arch = "wasm32")]
pub fn spawn<T, F>(name: impl Into<String>, _f: F) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("no threads in the browser build yet ({})", name.into()),
    ))
}

/// Runs a one-shot job in the background (a world scan, a file dialog).
/// Where there are no threads it runs on the calling thread, so the job
/// must not wait for anything the caller does later.
pub fn spawn_detached<F>(name: &str, f: F)
where
    F: FnOnce() + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(err) = std::thread::Builder::new().name(name.to_owned()).spawn(f) {
        log::warn!("couldn't start {name}: {err}");
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        f();
    }
}

/// How many threads worth of cores there are (1 in the browser until the
/// worker pool reports `hardwareConcurrency`, M72).
#[cfg(not(target_arch = "wasm32"))]
pub fn available_parallelism() -> io::Result<std::num::NonZeroUsize> {
    std::thread::available_parallelism()
}

#[cfg(target_arch = "wasm32")]
pub fn available_parallelism() -> io::Result<std::num::NonZeroUsize> {
    Ok(std::num::NonZeroUsize::MIN)
}

/// Lets other threads run for a moment.
#[cfg(not(target_arch = "wasm32"))]
pub fn sleep(d: std::time::Duration) {
    std::thread::sleep(d);
}

/// The main thread must not block in the browser, so this does nothing.
#[cfg(target_arch = "wasm32")]
pub fn sleep(_d: std::time::Duration) {}

/// Runs a future to completion on the calling thread. Native: waits for it.
/// The browser can't wait (the main thread never blocks), so there a future
/// must be ready at once; GPU setup, which isn't, is made `async` in M71.
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    pollster::block_on(f)
}

#[cfg(target_arch = "wasm32")]
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => v,
        std::task::Poll::Pending => panic!("block_on: the browser build can't wait for a future"),
    }
}

/// The browser's generator workers for a `ChunkManager` with no threads of
/// its own; `None` natively (it has threads) and in single thread mode.
#[cfg(not(target_arch = "wasm32"))]
pub fn gen_pool() -> Option<Box<dyn crate::world::offload::GenPool>> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn gen_pool() -> Option<Box<dyn crate::world::offload::GenPool>> {
    crate::web::workers::generators()
}

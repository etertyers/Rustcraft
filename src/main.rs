mod app;
mod assets;
mod config;
mod inventory;
mod player;
mod render;
mod world;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::init();
    let event_loop = EventLoop::new()?;
    let mut app = app::App::default();
    event_loop.run_app(&mut app)?;
    app.into_result()
}

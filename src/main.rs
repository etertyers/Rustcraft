mod app;
mod config;
mod player;
mod render;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::init();
    let event_loop = EventLoop::new()?;
    let mut app = app::App::default();
    event_loop.run_app(&mut app)?;
    app.into_result()
}

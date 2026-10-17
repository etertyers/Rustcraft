mod app;
mod assets;
mod config;
mod inventory;
mod player;
mod render;
mod ui;
mod world;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::init();
    let event_loop = EventLoop::new()?;
    let world = if std::env::args().any(|arg| arg == "--gallery") {
        app::WorldKind::Gallery
    } else {
        app::WorldKind::Flat
    };
    let mut app = app::App::new(world);
    event_loop.run_app(&mut app)?;
    app.into_result()
}

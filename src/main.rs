mod app;
mod assets;
mod audio;
mod config;
mod entity;
mod frame;
mod i18n;
mod input;
mod metrics;
mod net;
mod physics;
mod render;
mod ui;
mod world;

use winit::event_loop::EventLoop;

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = app::App::new();
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.take_error() {
        return Err(error);
    }
    Ok(())
}

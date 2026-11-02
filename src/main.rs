mod app;
mod assets;
mod audio;
mod config;
mod entity;
mod i18n;
mod input;
mod net;
mod physics;
mod render;
mod ui;
mod world;

fn main() {
    println!("client {}", env!("CARGO_PKG_VERSION"));
}

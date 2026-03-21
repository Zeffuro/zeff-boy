mod hardware;
mod rom_loader;
mod graphics;
mod app;
mod emulator;
mod debug;

use std::path::Path;
use env_logger::Env;
use crate::emulator::Emulator;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().collect();

    let emulator = if args.len() >= 2 {
        let path = Path::new(&args[1]);
        Some(Emulator::from_rom(path)?)
    } else {
        None
    };

    app::run(emulator)?;

    Ok(())
}
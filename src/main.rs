mod hardware;
mod rom_loader;
mod graphics;
mod app;
mod emulator;
mod debug;
mod settings;
mod audio;
mod input;
mod cli;

use std::path::Path;
use env_logger::Env;
use crate::emulator::Emulator;
use crate::settings::Settings;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let mut settings = Settings::load_or_default();
    let args = cli::parse_args()?;

    if let Some(mode) = args.mode_override {
        settings.hardware_mode_preference = mode;
    }

    if let Some(headless_opts) = args.headless {
        let path_arg = args.rom_path.ok_or("--headless requires a ROM path")?;
        return cli::run_headless(Path::new(&path_arg), settings.hardware_mode_preference, &headless_opts);
    }

    let emulator = args
        .rom_path
        .map(|path_arg| Emulator::from_rom_with_mode(Path::new(&path_arg), settings.hardware_mode_preference))
        .transpose()?;

    app::run(emulator, settings)?;

    Ok(())
}
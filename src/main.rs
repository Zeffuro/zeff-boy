mod hardware;

use hardware::cpu::CPU;
use hardware::mmu::MMU;
mod rom_loader;

use std::path::Path;
use env_logger::Env;

const ROM: &str = "test-roms/gb-test-roms/cpu_instrs/cpu_instrs.gb";

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let rom_path = Path::new(ROM);
    let rom = rom_loader::load_rom(rom_path).unwrap();

    log::info!("ROM loaded: {} bytes", rom.len());

    let mut mmu = match MMU::new(rom) {
        Ok(mmu) => mmu,
        Err(e) => {
            log::error!("Failed to parse ROM header: {}", e);
            return;
        }
    };

    log::info!("Game title: {}", mmu.header.title);
    log::info!("Cartridge type: {:?}", mmu.header.cartridge_type);
    log::info!("Publisher: {}", mmu.header.publisher()); // as implemented above

    let mut cpu = CPU::new(&mut mmu);

    for _ in 0..100 {
        cpu.step();
    }
}
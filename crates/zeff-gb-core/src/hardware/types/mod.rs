mod apu_constants;
mod cartridge_type;
pub mod constants;
mod cpu_state;
pub mod header_offsets;
mod ram_size;
mod rom_size;

pub mod hardware_mode;
mod new_licensee;
mod old_licensee;
mod timer_clock;

pub use cartridge_type::CartridgeType;
pub use cpu_state::CpuState;
pub use cpu_state::ImeState;
pub use ram_size::RamSize;
pub use rom_size::RomSize;
pub use timer_clock::TimerClock;

pub fn new_licensee_name(code: &str) -> &'static str {
    new_licensee::new_licensee_name(code)
}

pub fn old_licensee_name(code: u8) -> &'static str {
    old_licensee::old_licensee_name(code)
}

mod apu_constants;
mod cartridge_type;
pub(crate) mod constants;
mod cpu_state;
pub(crate) mod header_offsets;
mod ram_size;
mod rom_size;

pub(crate) mod hardware_mode;
mod new_licensee;
mod old_licensee;
mod timer_clock;

pub(crate) use cartridge_type::CartridgeType;
pub(crate) use cpu_state::CPUState;
pub(crate) use cpu_state::IMEState;
pub(crate) use ram_size::RamSize;
pub(crate) use rom_size::RomSize;
pub(crate) use timer_clock::TimerClock;

pub(crate) fn new_licensee_name(code: &str) -> &'static str {
	new_licensee::new_licensee_name(code)
}

pub(crate) fn old_licensee_name(code: u8) -> &'static str {
	old_licensee::old_licensee_name(code)
}


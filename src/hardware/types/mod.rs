mod cartridge_type;
mod rom_size;
mod ram_size;
mod cpu_flags;

pub(crate) mod header_constants;
pub(crate) mod new_licensee;
pub(crate) mod old_licensee;
pub(crate) mod memory_constants;
pub(crate) mod hardware_constants;
pub(crate) mod timer_clock;
pub(crate) mod hardware_mode;

pub(crate) use cartridge_type::CartridgeType;
pub(crate) use rom_size::RomSize;
pub(crate) use ram_size::RamSize;
pub(crate) use cpu_flags::IMEState;
pub(crate) use cpu_flags::CPUState;

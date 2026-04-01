use crate::hardware::cartridge::CartridgeDebugInfo;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use zeff_emu_common::debug::{WatchHit, WatchType};

pub struct WatchpointInfo {
    pub address: u16,
    pub watch_type: WatchType,
}

pub struct DebugInfo {
    pub pc: u16,
    pub sp: u16,
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,

    pub cycles: u64,
    pub ime: &'static str,
    pub cpu_state: &'static str,
    pub last_opcode: u8,
    pub last_opcode_pc: u16,

    pub fps: f64,
    pub speed_mode_label: &'static str,
    pub frames_in_flight: usize,
    pub ppu: PpuSnapshot,
    pub hardware_mode: HardwareMode,
    pub hardware_mode_preference: HardwareModePreference,

    pub div: u8,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,

    pub if_reg: u8,
    pub ie: u8,

    pub mem_around_pc: [(u16, u8); 32],

    pub recent_ops: Vec<(u16, u8, bool)>,
    pub breakpoints: Vec<u16>,
    pub watchpoints: Vec<WatchpointInfo>,
    pub hit_breakpoint: Option<u16>,
    pub hit_watchpoint: Option<WatchHit>,

    pub tilt_is_mbc7: bool,
    pub tilt_stick_controls_tilt: bool,
    pub tilt_left_stick: (f32, f32),
    pub tilt_keyboard: (f32, f32),
    pub tilt_mouse: (f32, f32),
    pub tilt_target: (f32, f32),
    pub tilt_smoothed: (f32, f32),
}

#[derive(Clone, Copy)]
pub struct PpuSnapshot {
    pub lcdc: u8,
    pub stat: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub wy: u8,
    pub wx: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
}

#[derive(Clone)]
pub struct RomInfoViewData {
    pub title: String,
    pub manufacturer: String,
    pub publisher: String,
    pub cartridge_type: String,
    pub rom_size: String,
    pub ram_size: String,
    pub cgb_flag: u8,
    pub sgb_flag: u8,
    pub is_cgb_compatible: bool,
    pub is_cgb_exclusive: bool,
    pub is_sgb_supported: bool,
    pub header_checksum_valid: bool,
    pub global_checksum_valid: bool,
    pub rom_crc32: u32,
    pub libretro_title: Option<String>,
    pub libretro_rom_name: Option<String>,
    pub hardware_mode: HardwareMode,
    pub cartridge_state: CartridgeDebugInfo,
}

pub type OpcodeLog = zeff_emu_common::debug::OpcodeLog<(u16, u8, bool)>;

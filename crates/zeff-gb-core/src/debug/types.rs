use crate::debug::breakpoints::{WatchHit, WatchType};
use crate::hardware::cartridge::CartridgeDebugInfo;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};

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

type OpcodeEntry = (u16, u8, bool);

const OPCODE_LOG_CAPACITY: usize = 32;
const OPCODE_LOG_MASK: usize = OPCODE_LOG_CAPACITY - 1;

pub struct OpcodeLog {
    entries: [OpcodeEntry; OPCODE_LOG_CAPACITY],
    cursor: usize,
    count: usize,
    pub enabled: bool,
}

impl std::fmt::Debug for OpcodeLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpcodeLog")
            .field("count", &self.count)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl OpcodeLog {
    pub fn new(_capacity: usize) -> Self {
        Self {
            entries: [(0, 0, false); OPCODE_LOG_CAPACITY],
            cursor: 0,
            count: 0,
            enabled: true,
        }
    }

    #[inline]
    pub fn push(&mut self, pc: u16, opcode: u8, is_cb: bool) {
        if !self.enabled {
            return;
        }
        self.entries[self.cursor] = (pc, opcode, is_cb);
        self.cursor = (self.cursor + 1) & OPCODE_LOG_MASK;
        if self.count < OPCODE_LOG_CAPACITY {
            self.count += 1;
        }
    }

    pub fn recent(&self, n: usize) -> Vec<(u16, u8, bool)> {
        let take = n.min(self.count);
        let mut result = Vec::with_capacity(take);
        for i in 0..take {
            let idx = (self.cursor.wrapping_sub(1 + i)) & OPCODE_LOG_MASK;
            result.push(self.entries[idx]);
        }
        result
    }
}


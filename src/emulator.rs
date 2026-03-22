use crate::debug::{DebugController, DebugInfo, OpcodeLog, PpuSnapshot};
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::hardware::types::{CPUState, IMEState};
use crate::hardware::{
    bus::{Bus, CpuAccessTraceEvent},
    cpu::CPU,
};
use crate::rom_loader;
use crate::save_state::{
    SAVE_STATE_VERSION, SaveStateRef, read_from_file, slot_path, validate_compatibility,
    write_to_file,
};
use anyhow::Result as AnyResult;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const CYCLES_PER_FRAME_NORMAL: u64 = 70224;
const CYCLES_PER_FRAME_DOUBLE: u64 = 140448;
type RegisterSeed = (u8, u8, u8, u8, u8, u8, u8, u8);

const DMG_POST_BOOT_REGISTERS: RegisterSeed = (0x01, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D);
const CGB_POST_BOOT_REGISTERS: RegisterSeed = (0x11, 0x80, 0x00, 0x00, 0xFF, 0x56, 0x00, 0x0D);

pub(crate) struct Emulator {
    pub(crate) cpu: CPU,
    pub(crate) bus: Box<Bus>,
    pub(crate) header: RomHeader,
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) cycle_count: u64,
    pub(crate) opcode_log: OpcodeLog,
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,
    pub(crate) debug: DebugController,
    pub(crate) rom_hash: [u8; 32],
    rom_path: PathBuf,
}

impl Emulator {
    pub(crate) fn cycles_per_frame(mode: HardwareMode) -> u64 {
        if mode == HardwareMode::CGBDouble {
            CYCLES_PER_FRAME_DOUBLE
        } else {
            CYCLES_PER_FRAME_NORMAL
        }
    }

    fn post_boot_registers_for_mode(mode: HardwareMode) -> RegisterSeed {
        match mode {
            HardwareMode::CGBNormal | HardwareMode::CGBDouble => CGB_POST_BOOT_REGISTERS,
            HardwareMode::DMG | HardwareMode::SGB1 | HardwareMode::SGB2 => DMG_POST_BOOT_REGISTERS,
        }
    }

    pub(crate) fn from_rom_with_mode(
        path: &Path,
        mode_preference: HardwareModePreference,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let rom = rom_loader::load_rom(path)?;
        let rom_hash = Self::compute_rom_hash(&rom);
        log::info!("ROM loaded: {} bytes", rom.len());

        let header = RomHeader::from_rom(&rom)?;
        header.display_info(&rom);
        let hardware_mode = mode_preference.resolve(
            header.is_cgb_compatible,
            header.is_sgb_supported,
            header.old_licensee_code,
        );
        if matches!(mode_preference, HardwareModePreference::ForceCgb) && !header.is_cgb_compatible
        {
            log::warn!(
                "ForceCgb requested for DMG-only ROM; falling back to DMG mode for compatibility"
            );
        }
        let bus = Bus::new(rom, &header, hardware_mode)?;

        let mut emulator = Self {
            cpu: CPU::new(),
            bus,
            header,
            hardware_mode_preference: mode_preference,
            hardware_mode,
            cycle_count: 0,
            opcode_log: OpcodeLog::new(32),
            last_opcode: 0,
            last_opcode_pc: 0,
            debug: DebugController::new(),
            rom_hash,
            rom_path: path.to_path_buf(),
        };

        emulator.apply_post_boot_state();
        if let Some(sram_path) = emulator.try_load_battery_sram()? {
            log::info!("Loaded battery save from {}", sram_path);
        }
        Ok(emulator)
    }

    pub(crate) fn flush_battery_sram(&self) -> AnyResult<Option<String>> {
        if !self.header.cartridge_type.is_battery_backed() {
            return Ok(None);
        }

        let sram = self.bus.cartridge.dump_sram();
        if sram.is_empty() {
            return Ok(None);
        }

        let save_path = rom_loader::save_file_path_for_rom(&self.rom_path);
        rom_loader::write_save_file(&save_path, &sram)?;
        Ok(Some(save_path.display().to_string()))
    }

    fn try_load_battery_sram(&mut self) -> AnyResult<Option<String>> {
        if !self.header.cartridge_type.is_battery_backed() {
            return Ok(None);
        }

        let expected_len = self.bus.cartridge.sram_len();
        if expected_len == 0 {
            return Ok(None);
        }

        let save_path = rom_loader::save_file_path_for_rom(&self.rom_path);
        if !save_path.exists() {
            return Ok(None);
        }

        let loaded = rom_loader::load_save_file(&save_path)?;
        if loaded.len() != expected_len {
            log::warn!(
                "SRAM size mismatch for {}: got {} bytes, expected {} (will truncate/pad)",
                save_path.display(),
                loaded.len(),
                expected_len
            );
        }

        let mut adjusted = vec![0u8; expected_len];
        let copy_len = expected_len.min(loaded.len());
        adjusted[..copy_len].copy_from_slice(&loaded[..copy_len]);
        self.bus.cartridge.load_sram(&adjusted);

        Ok(Some(save_path.display().to_string()))
    }

    fn compute_rom_hash(rom: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(rom);
        hasher.finalize().into()
    }

    pub(crate) fn rom_path(&self) -> &Path {
        &self.rom_path
    }

    pub(crate) fn save_state(&self, slot: u8) -> AnyResult<String> {
        let path = slot_path(self.rom_hash, slot)?;
        self.save_state_to_path(&path)?;
        Ok(path.display().to_string())
    }

    pub(crate) fn save_state_to_path(&self, path: &Path) -> AnyResult<()> {
        let state = SaveStateRef {
            version: SAVE_STATE_VERSION,
            rom_hash: self.rom_hash,
            cpu: &self.cpu,
            bus: self.bus.as_ref(),
            hardware_mode_preference: self.hardware_mode_preference,
            hardware_mode: self.hardware_mode,
            cycle_count: self.cycle_count,
            last_opcode: self.last_opcode,
            last_opcode_pc: self.last_opcode_pc,
        };
        write_to_file(&path, &state)?;
        Ok(())
    }

    pub(crate) fn load_state(&mut self, slot: u8) -> AnyResult<String> {
        let path = slot_path(self.rom_hash, slot)?;
        self.load_state_from_path(&path)?;
        Ok(path.display().to_string())
    }

    pub(crate) fn load_state_from_path(&mut self, path: &Path) -> AnyResult<()> {
        let state = read_from_file(&path)?;
        validate_compatibility(&state, self.rom_hash)?;

        let current_sample_rate = self.bus.io.apu.sample_rate;
        let rom_bytes = self.bus.cartridge.rom_bytes().to_vec();
        let mut restored_bus = state.bus;
        restored_bus.cartridge.restore_rom_bytes(rom_bytes);
        restored_bus.io.timer.mode = restored_bus.hardware_mode;
        restored_bus.io.serial.mode = restored_bus.hardware_mode;
        restored_bus.io.apu.set_sample_rate(current_sample_rate);
        restored_bus.io.ppu.set_sgb_mode(matches!(
            restored_bus.hardware_mode,
            HardwareMode::SGB1 | HardwareMode::SGB2
        ));

        self.cpu = state.cpu;
        self.bus = Box::new(restored_bus);
        self.hardware_mode_preference = state.hardware_mode_preference;
        self.hardware_mode = state.hardware_mode;
        self.cycle_count = state.cycle_count;
        self.last_opcode = state.last_opcode;
        self.last_opcode_pc = state.last_opcode_pc;

        self.opcode_log = OpcodeLog::new(32);
        self.debug = DebugController::new();
        self.bus.trace_cpu_accesses = false;
        self.bus.begin_cpu_access_trace();

        Ok(())
    }

    fn apply_post_boot_state(&mut self) {
        self.cpu.pc = 0x0100;
        self.cpu.sp = 0xFFFE;

        let (a, f, b, c, d, e, h, l) = Self::post_boot_registers_for_mode(self.hardware_mode);
        self.cpu.a = a;
        self.cpu.f = f;
        self.cpu.b = b;
        self.cpu.c = c;
        self.cpu.d = d;
        self.cpu.e = e;
        self.cpu.h = h;
        self.cpu.l = l;
    }

    pub(crate) fn step_instruction(&mut self) -> (u16, u8, bool, u64) {
        if matches!(self.cpu.running, CPUState::Suspended) {
            return (self.cpu.pc, self.bus.read_byte(self.cpu.pc), false, 0);
        }

        let watch_active = self.debug.has_watchpoints();
        self.bus.trace_cpu_accesses = watch_active;
        if watch_active {
            self.bus.begin_cpu_access_trace();
        }

        let pc_before = self.cpu.pc;
        let opcode_at_pc = self.bus.read_byte(pc_before);

        self.cpu.step(&mut self.bus);

        self.hardware_mode = self.bus.hardware_mode;

        if watch_active {
            let hit_watchpoint = {
                let debug = &mut self.debug;
                self.bus.drain_cpu_access_trace(|event| match event {
                    CpuAccessTraceEvent::Read { addr, value } => {
                        debug.check_watch_read(addr, value)
                    }
                    CpuAccessTraceEvent::Write {
                        addr,
                        old_value,
                        new_value,
                    } => debug.check_watch_write(addr, old_value, new_value),
                });
                debug.hit_watchpoint.is_some()
            };

            if hit_watchpoint {
                self.cpu.running = CPUState::Suspended;
            }
        }

        if self.debug.should_break(pc_before) {
            self.cpu.running = CPUState::Suspended;
        }

        let (op, cb_prefix) = if opcode_at_pc == 0xCB {
            (self.bus.read_byte(pc_before.wrapping_add(1)), true)
        } else {
            (opcode_at_pc, false)
        };

        self.opcode_log.push(pc_before, op, cb_prefix);
        self.last_opcode = op;
        self.last_opcode_pc = pc_before;

        debug_assert_eq!(
            self.cpu.timed_cycles_accounted, self.cpu.last_step_cycles,
            "peripheral timing is expected to be fully CPU-driven (pc={:#06X}, opcode={:#04X}, cb_prefix={})",
            pc_before, opcode_at_pc, cb_prefix
        );

        (pc_before, op, cb_prefix, self.cpu.last_step_cycles)
    }

    pub(crate) fn step_frame(&mut self) {
        if matches!(self.cpu.running, CPUState::Suspended) {
            return;
        }

        let frame_cycles = Self::cycles_per_frame(self.hardware_mode);
        let target = self.cpu.cycles.wrapping_add(frame_cycles);

        while self.cpu.cycles < target && !matches!(self.cpu.running, CPUState::Suspended) {
            let _ = self.step_instruction();
        }
    }

    pub(crate) fn framebuffer(&self) -> &[u8] {
        &self.bus.io.ppu.framebuffer
    }

    pub(crate) fn vram(&self) -> &[u8] {
        &self.bus.vram
    }

    pub(crate) fn oam(&self) -> &[u8] {
        &self.bus.oam
    }

    pub(crate) fn rom_info(&self) -> &RomHeader {
        &self.header
    }

    pub(crate) fn cartridge_state(&self) -> crate::hardware::cartridge::CartridgeDebugInfo {
        self.bus.cartridge.debug_info()
    }

    pub(crate) fn read_memory_range(&self, start: u16, len: u16) -> Vec<(u16, u8)> {
        (0..len)
            .map(|i| {
                let addr = start.wrapping_add(i);
                (addr, self.bus.read_byte(addr))
            })
            .collect()
    }

    pub(crate) fn ppu_registers(&self) -> PpuSnapshot {
        PpuSnapshot {
            lcdc: self.bus.io.ppu.lcdc,
            stat: self.bus.io.ppu.stat,
            scy: self.bus.io.ppu.scy,
            scx: self.bus.io.ppu.scx,
            ly: self.bus.io.ppu.ly,
            lyc: self.bus.io.ppu.lyc,
            wy: self.bus.io.ppu.wy,
            wx: self.bus.io.ppu.wx,
            bgp: self.bus.io.ppu.bgp,
            obp0: self.bus.io.ppu.obp0,
            obp1: self.bus.io.ppu.obp1,
        }
    }

    pub(crate) fn snapshot(&self) -> DebugInfo {
        let ime = match self.cpu.ime {
            IMEState::Enabled => "Enabled",
            IMEState::Disabled => "Disabled",
            IMEState::PendingEnable => "Pending",
        };
        let cpu_state = match self.cpu.running {
            CPUState::Running => "Running",
            CPUState::Halted => "Halted",
            CPUState::Stopped => "Stopped",
            CPUState::InterruptHandling => "IntHandle",
            CPUState::Reset => "Reset",
            CPUState::Suspended => "Suspended",
        };

        let start = self.cpu.pc;
        let mut mem_around_pc = Vec::with_capacity(32);
        for i in 0u16..32 {
            let addr = start.wrapping_add(i);
            mem_around_pc.push((addr, self.bus.read_byte(addr)));
        }

        let speed_mode_label = match self.hardware_mode {
            HardwareMode::CGBDouble => "Double",
            _ => "Normal",
        };

        DebugInfo {
            pc: self.cpu.pc,
            sp: self.cpu.sp,
            a: self.cpu.a,
            f: self.cpu.f,
            b: self.cpu.b,
            c: self.cpu.c,
            d: self.cpu.d,
            e: self.cpu.e,
            h: self.cpu.h,
            l: self.cpu.l,
            cycles: self.cpu.cycles,
            ime,
            cpu_state,
            last_opcode: self.last_opcode,
            last_opcode_pc: self.last_opcode_pc,
            fps: 0.0,
            speed_mode_label,
            ppu: self.ppu_registers(),
            hardware_mode: self.hardware_mode,
            hardware_mode_preference: self.hardware_mode_preference,
            div: self.bus.io.timer.div,
            tima: self.bus.io.timer.tima,
            tma: self.bus.io.timer.tma,
            tac: self.bus.io.timer.tac,
            if_reg: self.bus.if_reg,
            ie: self.bus.ie,
            mem_around_pc,
            recent_ops: self.opcode_log.recent(16),
            breakpoints: {
                let mut points: Vec<u16> = self.debug.breakpoints.iter().copied().collect();
                points.sort_unstable();
                points
            },
            watchpoints: self
                .debug
                .watchpoints
                .iter()
                .map(|w| format!("{:04X} ({:?})", w.address, w.watch_type))
                .collect(),
            hit_breakpoint: self.debug.hit_breakpoint,
            hit_watchpoint: self.debug.hit_watchpoint.map(|hit| {
                format!(
                    "{:?} @ {:04X}: {:02X} -> {:02X}",
                    hit.watch_type, hit.address, hit.old_value, hit.new_value
                )
            }),
        }
    }
}
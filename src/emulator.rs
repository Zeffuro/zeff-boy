use crate::debug::{DebugController, DebugInfo, OpcodeLog, PpuSnapshot};
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::hardware::types::{CPUState, IMEState};
use crate::hardware::{
    bus::{Bus, CpuAccessTraceEvent},
    cpu::CPU,
};
use crate::rom_loader;
use std::path::Path;

const CYCLES_PER_FRAME: u64 = 70224;
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
}

impl Emulator {
    pub(crate) fn cycles_per_frame() -> u64 {
        CYCLES_PER_FRAME
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
        };

        emulator.apply_post_boot_state();
        Ok(emulator)
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
        let target = self.cpu.cycles.wrapping_add(CYCLES_PER_FRAME);
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
            speed_mode_label: "Normal",
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

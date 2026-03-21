use std::path::Path;
use crate::hardware::{cpu::CPU, bus::Bus};
use crate::hardware::rom_header::RomHeader;
use crate::rom_loader;
use crate::debug::{DebugInfo, OpcodeLog, PpuSnapshot};
use crate::hardware::types::{IMEState, CPUState};

const CYCLES_PER_FRAME: u64 = 70224;

pub(crate) struct Emulator {
    pub(crate) cpu: CPU,
    pub(crate) bus: Box<Bus>,
    pub(crate) header: RomHeader,
    pub(crate) cycle_count: u64,
    pub(crate) opcode_log: OpcodeLog,
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,
}

impl Emulator {
    pub(crate) fn from_rom(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let rom = rom_loader::load_rom(path)?;
        log::info!("ROM loaded: {} bytes", rom.len());

        let header = RomHeader::from_rom(&rom)?;
        header.display_info();
        let bus = Bus::new(rom, &header)?;

        Ok(Self {
            cpu: CPU::new(),
            bus,
            header,
            cycle_count: 0,
            opcode_log: OpcodeLog::new(32),
            last_opcode: 0,
            last_opcode_pc: 0,
        })
    }

    pub(crate) fn step_frame(&mut self) {
        let target = self.cpu.cycles + CYCLES_PER_FRAME;
        while self.cpu.cycles < target {
            let pc_before = self.cpu.pc;
            let opcode_at_pc = self.bus.read_byte(pc_before);

            self.cpu.step(&mut self.bus);

            if opcode_at_pc == 0xCB {
                let cb_op = self.bus.read_byte(pc_before.wrapping_add(1));
                self.opcode_log.push(pc_before, cb_op, true);
                self.last_opcode = cb_op;
            } else {
                self.opcode_log.push(pc_before, opcode_at_pc, false);
                self.last_opcode = opcode_at_pc;
            }
            self.last_opcode_pc = pc_before;

            let cycles = self.cpu.last_step_cycles;
            if self.bus.io.timer.step(cycles) {
                self.bus.if_reg |= 0x04;
            }
            if self.bus.io.serial.step(cycles) {
                self.bus.if_reg |= 0x08;
            }
            self.bus.if_reg |= self.bus.io.ppu.step(cycles, &self.bus.vram, &self.bus.oam);
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
            ly: self.bus.io.ppu.ly,
            lcdc: self.bus.io.ppu.lcdc,
            stat: self.bus.io.ppu.stat,
            div: self.bus.io.timer.div,
            tima: self.bus.io.timer.tima,
            tma: self.bus.io.timer.tma,
            tac: self.bus.io.timer.tac,
            if_reg: self.bus.if_reg,
            ie: self.bus.ie,
            mem_around_pc,
            recent_ops: self.opcode_log.recent(16),
        }
    }
}
use super::Emulator;
use crate::debug::{DebugInfo, PpuSnapshot, WatchpointInfo};
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::types::{CPUState, IMEState};

impl Emulator {
    pub(crate) fn framebuffer(&self) -> &[u8] {
        &self.bus.io.ppu.framebuffer
    }

    pub(crate) fn vram(&self) -> &[u8] {
        &self.bus.vram
    }

    pub(crate) fn oam(&self) -> &[u8] {
        &self.bus.oam
    }

    pub(crate) fn rom_info(&self) -> &crate::hardware::rom_header::RomHeader {
        &self.header
    }

    pub(crate) fn cartridge_state(&self) -> crate::hardware::cartridge::CartridgeDebugInfo {
        self.bus.cartridge.debug_info()
    }

    pub(crate) fn is_mbc7_cartridge(&self) -> bool {
        self.header.cartridge_type.is_mbc7()
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
        let mut mem_around_pc = [(0u16, 0u8); 32];
        for (i, entry) in mem_around_pc.iter_mut().enumerate() {
            let addr = start.wrapping_add(i as u16);
            *entry = (addr, self.bus.read_byte(addr));
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
            frames_in_flight: 0,
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
                .map(|w| WatchpointInfo {
                    address: w.address,
                    watch_type: w.watch_type,
                })
                .collect(),
            hit_breakpoint: self.debug.hit_breakpoint,
            hit_watchpoint: self.debug.hit_watchpoint,
            tilt_is_mbc7: false,
            tilt_stick_controls_tilt: false,
            tilt_left_stick: (0.0, 0.0),
            tilt_keyboard: (0.0, 0.0),
            tilt_mouse: (0.0, 0.0),
            tilt_target: (0.0, 0.0),
            tilt_smoothed: (0.0, 0.0),
        }
    }
}

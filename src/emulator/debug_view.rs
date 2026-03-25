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
                let mut points: Vec<u16> = self.debug.iter_breakpoints().collect();
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

    #[allow(dead_code)]
    pub(crate) fn debug_state_summary(&self) -> String {
        let ppu = &self.bus.io.ppu;
        let timer = &self.bus.io.timer;
        let cart = self.bus.cartridge.debug_info();
        format!(
            "=== Emulator State ===\n\
             ROM: {title}\n\
             Mode: {mode:?} (pref: {pref:?})\n\
             Cycles: {cycles}\n\
             \n\
             --- CPU ---\n\
             PC={pc:#06X}  SP={sp:#06X}\n\
             AF={a:02X}{f:02X}  BC={b:02X}{c:02X}  DE={d:02X}{e:02X}  HL={h:02X}{l:02X}\n\
             IME={ime:?}  State={state:?}  HaltBug={hb}\n\
             Last op: {lop:#04X} @ {lopc:#06X}\n\
             \n\
             --- PPU ---\n\
             LCDC={lcdc:#04X}  STAT={stat:#04X}  LY={ly}  LYC={lyc}\n\
             SCX={scx}  SCY={scy}  WX={wx}  WY={wy}\n\
             BGP={bgp:#04X}  OBP0={obp0:#04X}  OBP1={obp1:#04X}\n\
             PPU cycles: {ppuc}\n\
             \n\
             --- Timer ---\n\
             DIV={div:#04X}  TIMA={tima:#04X}  TMA={tma:#04X}  TAC={tac:#04X}\n\
             \n\
             --- Interrupts ---\n\
             IE={ie:#04X}  IF={if_reg:#04X}\n\
             \n\
             --- Cartridge ---\n\
             Mapper: {mapper}  ROM bank: {rbank}  RAM bank: {abank}  RAM enabled: {ram_en}",
            title = self.header.title,
            mode = self.hardware_mode,
            pref = self.hardware_mode_preference,
            cycles = self.cycle_count,
            pc = self.cpu.pc,
            sp = self.cpu.sp,
            a = self.cpu.a,
            f = self.cpu.f,
            b = self.cpu.b,
            c = self.cpu.c,
            d = self.cpu.d,
            e = self.cpu.e,
            h = self.cpu.h,
            l = self.cpu.l,
            ime = self.cpu.ime,
            state = self.cpu.running,
            hb = self.cpu.halt_bug_active,
            lop = self.last_opcode,
            lopc = self.last_opcode_pc,
            lcdc = ppu.lcdc,
            stat = ppu.stat,
            ly = ppu.ly,
            lyc = ppu.lyc,
            scx = ppu.scx,
            scy = ppu.scy,
            wx = ppu.wx,
            wy = ppu.wy,
            bgp = ppu.bgp,
            obp0 = ppu.obp0,
            obp1 = ppu.obp1,
            ppuc = ppu.cycles,
            div = timer.div,
            tima = timer.tima,
            tma = timer.tma,
            tac = timer.tac,
            ie = self.bus.ie,
            if_reg = self.bus.if_reg,
            mapper = cart.mapper,
            rbank = cart.active_rom_bank,
            abank = cart.active_ram_bank,
            ram_en = cart.ram_enabled,
        )
    }
}

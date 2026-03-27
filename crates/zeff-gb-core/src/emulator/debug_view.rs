use super::Emulator;
use crate::debug::{DebugInfo, PpuSnapshot, WatchpointInfo};
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::types::{CpuState, ImeState};

impl Emulator {
    pub fn framebuffer(&self) -> &[u8] {
        self.bus.ppu_framebuffer()
    }

    pub fn vram(&self) -> &[u8] {
        &self.bus.vram
    }

    pub fn oam(&self) -> &[u8] {
        &self.bus.oam
    }

    pub fn rom_info(&self) -> &crate::hardware::rom_header::RomHeader {
        &self.header
    }

    pub fn cartridge_state(&self) -> crate::hardware::cartridge::CartridgeDebugInfo {
        self.bus.cartridge.debug_info()
    }

    pub fn is_mbc7_cartridge(&self) -> bool {
        self.header.cartridge_type.is_mbc7()
    }

    pub fn ppu_registers(&self) -> PpuSnapshot {
        PpuSnapshot {
            lcdc: self.bus.ppu_lcdc(),
            stat: self.bus.ppu_stat(),
            scy: self.bus.ppu_scy(),
            scx: self.bus.ppu_scx(),
            ly: self.bus.ppu_ly(),
            lyc: self.bus.ppu_lyc(),
            wy: self.bus.ppu_wy(),
            wx: self.bus.ppu_wx(),
            bgp: self.bus.ppu_bgp(),
            obp0: self.bus.ppu_obp0(),
            obp1: self.bus.ppu_obp1(),
        }
    }

    pub fn snapshot(&self) -> DebugInfo {
        let ime = match self.cpu.ime {
            ImeState::Enabled => "Enabled",
            ImeState::Disabled => "Disabled",
            ImeState::PendingEnable => "Pending",
        };
        let cpu_state = match self.cpu.running {
            CpuState::Running => "Running",
            CpuState::Halted => "Halted",
            CpuState::Stopped => "Stopped",
            CpuState::InterruptHandling => "IntHandle",
            CpuState::Reset => "Reset",
            CpuState::Suspended => "Suspended",
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
            a: self.cpu.regs.a,
            f: self.cpu.regs.f,
            b: self.cpu.regs.b,
            c: self.cpu.regs.c,
            d: self.cpu.regs.d,
            e: self.cpu.regs.e,
            h: self.cpu.regs.h,
            l: self.cpu.regs.l,
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
            div: self.bus.timer_div(),
            tima: self.bus.timer_tima(),
            tma: self.bus.timer_tma(),
            tac: self.bus.timer_tac(),
            if_reg: self.bus.if_reg,
            ie: self.bus.ie,
            mem_around_pc,
            recent_ops: self.opcode_log.recent(32),
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
    pub fn debug_state_summary(&self) -> String {
        let cart = self.bus.cartridge.debug_info();
        format!(
            "=== Emulator State ===\n\
             ROM: {title}\n\
             Mode: {mode:?} (pref: {pref:?})\n\
             Cycles: {cycles}\n\
             \n\
             --- Cpu ---\n\
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
            a = self.cpu.regs.a,
            f = self.cpu.regs.f,
            b = self.cpu.regs.b,
            c = self.cpu.regs.c,
            d = self.cpu.regs.d,
            e = self.cpu.regs.e,
            h = self.cpu.regs.h,
            l = self.cpu.regs.l,
            ime = self.cpu.ime,
            state = self.cpu.running,
            hb = self.cpu.halt_bug_active,
            lop = self.last_opcode,
            lopc = self.last_opcode_pc,
            lcdc = self.bus.ppu_lcdc(),
            stat = self.bus.ppu_stat(),
            ly = self.bus.ppu_ly(),
            lyc = self.bus.ppu_lyc(),
            scx = self.bus.ppu_scx(),
            scy = self.bus.ppu_scy(),
            wx = self.bus.ppu_wx(),
            wy = self.bus.ppu_wy(),
            bgp = self.bus.ppu_bgp(),
            obp0 = self.bus.ppu_obp0(),
            obp1 = self.bus.ppu_obp1(),
            ppuc = self.bus.ppu_cycles(),
            div = self.bus.timer_div(),
            tima = self.bus.timer_tima(),
            tma = self.bus.timer_tma(),
            tac = self.bus.timer_tac(),
            ie = self.bus.ie,
            if_reg = self.bus.if_reg,
            mapper = cart.mapper,
            rbank = cart.active_rom_bank,
            abank = cart.active_ram_bank,
            ram_en = cart.ram_enabled,
        )
    }
}

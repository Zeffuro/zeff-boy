use crate::hardware::cpu::registers::*;

const OPCODE_LOG_CAPACITY: usize = 32;
const OPCODE_LOG_MASK: usize = OPCODE_LOG_CAPACITY - 1;

pub struct OpcodeLog {
    entries: [(u16, u8); OPCODE_LOG_CAPACITY],
    cursor: usize,
    count: usize,
    pub(crate) enabled: bool,
}

impl std::fmt::Debug for OpcodeLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpcodeLog")
            .field("count", &self.count)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl Default for OpcodeLog {
    fn default() -> Self {
        Self::new()
    }
}

impl OpcodeLog {
    pub fn new() -> Self {
        Self {
            entries: [(0, 0); OPCODE_LOG_CAPACITY],
            cursor: 0,
            count: 0,
            enabled: true,
        }
    }

    #[inline]
    pub fn push(&mut self, pc: u16, opcode: u8) {
        if !self.enabled {
            return;
        }
        self.entries[self.cursor] = (pc, opcode);
        self.cursor = (self.cursor + 1) & OPCODE_LOG_MASK;
        if self.count < OPCODE_LOG_CAPACITY {
            self.count += 1;
        }
    }

    pub fn recent(&self, n: usize) -> Vec<(u16, u8)> {
        let take = n.min(self.count);
        let mut result = Vec::with_capacity(take);
        for i in 0..take {
            let idx = (self.cursor.wrapping_sub(1 + i)) & OPCODE_LOG_MASK;
            result.push(self.entries[idx]);
        }
        result
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.cursor = 0;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchType {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Debug)]
pub struct Watchpoint {
    pub address: u16,
    pub watch_type: WatchType,
    pub last_value: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct WatchHit {
    pub address: u16,
    pub old_value: u8,
    pub new_value: u8,
    pub watch_type: WatchType,
}

pub struct DebugController {
    breakpoints: Box<[bool; 65536]>,
    breakpoint_count: usize,
    pub watchpoints: Vec<Watchpoint>,
    pub break_on_next: bool,
    pub hit_breakpoint: Option<u16>,
    pub hit_watchpoint: Option<WatchHit>,
    breakpoints_active: bool,
    watchpoints_active: bool,
}

impl std::fmt::Debug for DebugController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebugController")
            .field("breakpoint_count", &self.breakpoint_count)
            .field("watchpoints", &self.watchpoints)
            .field("break_on_next", &self.break_on_next)
            .field("hit_breakpoint", &self.hit_breakpoint)
            .field("hit_watchpoint", &self.hit_watchpoint)
            .finish_non_exhaustive()
    }
}

impl Default for DebugController {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugController {
    pub fn new() -> Self {
        Self {
            breakpoints: Box::new([false; 65536]),
            breakpoint_count: 0,
            watchpoints: Vec::new(),
            break_on_next: false,
            hit_breakpoint: None,
            hit_watchpoint: None,
            breakpoints_active: false,
            watchpoints_active: false,
        }
    }

    #[allow(dead_code)]
    pub fn has_breakpoint(&self, addr: u16) -> bool {
        self.breakpoints[addr as usize]
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.breakpoints
            .iter()
            .enumerate()
            .filter(|entry| *entry.1)
            .map(|entry| entry.0 as u16)
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        if !self.breakpoints[addr as usize] {
            self.breakpoints[addr as usize] = true;
            self.breakpoint_count += 1;
        }
        self.breakpoints_active = true;
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        if self.breakpoints[addr as usize] {
            self.breakpoints[addr as usize] = false;
            self.breakpoint_count -= 1;
        }
        self.breakpoints_active = self.breakpoint_count > 0;
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        let slot = &mut self.breakpoints[addr as usize];
        if *slot {
            *slot = false;
            self.breakpoint_count -= 1;
        } else {
            *slot = true;
            self.breakpoint_count += 1;
        }
        self.breakpoints_active = self.breakpoint_count > 0;
    }

    pub fn add_watchpoint(&mut self, addr: u16, watch_type: WatchType) {
        if self
            .watchpoints
            .iter()
            .any(|w| w.address == addr && w.watch_type == watch_type)
        {
            return;
        }
        self.watchpoints.push(Watchpoint {
            address: addr,
            watch_type,
            last_value: None,
        });
        self.watchpoints_active = true;
    }

    #[inline]
    pub fn should_break(&mut self, pc: u16) -> bool {
        if !self.breakpoints_active && !self.break_on_next {
            return false;
        }

        if self.breakpoints[pc as usize] {
            self.hit_breakpoint = Some(pc);
            self.break_on_next = false;
            return true;
        }

        if self.break_on_next {
            self.break_on_next = false;
            return true;
        }

        false
    }

    #[inline]
    pub fn has_watchpoints(&self) -> bool {
        self.watchpoints_active
    }

    pub fn check_watch_read(&mut self, addr: u16, value: u8) {
        if self.watchpoints.is_empty() {
            return;
        }

        for watch in &mut self.watchpoints {
            if watch.address != addr {
                continue;
            }
            if matches!(watch.watch_type, WatchType::Read | WatchType::ReadWrite) {
                let old_value = watch.last_value.unwrap_or(value);
                watch.last_value = Some(value);
                self.hit_watchpoint = Some(WatchHit {
                    address: addr,
                    old_value,
                    new_value: value,
                    watch_type: WatchType::Read,
                });
                return;
            }
        }
    }

    pub fn check_watch_write(&mut self, addr: u16, old_val: u8, new_val: u8) {
        if self.watchpoints.is_empty() {
            return;
        }

        for watch in &mut self.watchpoints {
            if watch.address != addr {
                continue;
            }
            if matches!(watch.watch_type, WatchType::Write | WatchType::ReadWrite)
                && old_val != new_val
            {
                watch.last_value = Some(new_val);
                self.hit_watchpoint = Some(WatchHit {
                    address: addr,
                    old_value: old_val,
                    new_value: new_val,
                    watch_type: WatchType::Write,
                });
                return;
            }
        }
    }

    pub fn clear_hits(&mut self) {
        self.hit_breakpoint = None;
        self.hit_watchpoint = None;
    }
}

#[derive(Clone)]
pub struct NesDebugSnapshot {
    pub pc: u16,
    pub sp: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,
    pub cycles: u64,
    pub cpu_state: &'static str,
    pub last_opcode: u8,
    pub last_opcode_pc: u16,
    pub nmi_pending: bool,
    pub irq_line: bool,

    pub ppu_scanline: u16,
    pub ppu_dot: u16,
    pub ppu_ctrl: u8,
    pub ppu_mask: u8,
    pub ppu_status: u8,
    pub ppu_v: u16,
    pub ppu_t: u16,
    pub ppu_fine_x: u8,
    pub ppu_in_vblank: bool,
    pub ppu_frame_count: u64,

    pub mem_around_pc: [(u16, u8); 32],

    pub recent_ops: Vec<(u16, u8)>,

    pub flag_n: bool,
    pub flag_v: bool,
    pub flag_d: bool,
    pub flag_i: bool,
    pub flag_z: bool,
    pub flag_c: bool,
}

impl NesDebugSnapshot {
    pub fn capture(emu: &crate::emulator::Emulator) -> Self {
        let cpu = &emu.cpu;
        let ppu = &emu.bus.ppu;

        let cpu_state = match cpu.state {
            crate::hardware::cpu::CpuState::Running => "Running",
            crate::hardware::cpu::CpuState::Halted => "Halted",
            crate::hardware::cpu::CpuState::Suspended => "Suspended",
        };

        let mut mem = [(0u16, 0u8); 32];
        for (i, entry) in mem.iter_mut().enumerate() {
            let addr = cpu.pc.wrapping_add(i as u16);

            entry.0 = addr;
            entry.1 = peek_byte(&emu.bus, addr);
        }

        Self {
            pc: cpu.pc,
            sp: cpu.sp,
            a: cpu.regs.a,
            x: cpu.regs.x,
            y: cpu.regs.y,
            p: cpu.regs.p,
            cycles: cpu.cycles,
            cpu_state,
            last_opcode: cpu.last_opcode,
            last_opcode_pc: cpu.last_opcode_pc,
            nmi_pending: cpu.nmi_pending,
            irq_line: cpu.irq_line,

            ppu_scanline: ppu.scanline,
            ppu_dot: ppu.dot,
            ppu_ctrl: ppu.regs.ctrl,
            ppu_mask: ppu.regs.mask,
            ppu_status: ppu.regs.status,
            ppu_v: ppu.v,
            ppu_t: ppu.t,
            ppu_fine_x: ppu.fine_x,
            ppu_in_vblank: ppu.in_vblank,
            ppu_frame_count: ppu.frame_count,

            mem_around_pc: mem,

            recent_ops: emu.opcode_log.recent(16),

            flag_n: cpu.regs.get_flag(NEGATIVE_FLAG),
            flag_v: cpu.regs.get_flag(OVERFLOW_FLAG),
            flag_d: cpu.regs.get_flag(DECIMAL_FLAG),
            flag_i: cpu.regs.get_flag(INTERRUPT_FLAG),
            flag_z: cpu.regs.get_flag(ZERO_FLAG),
            flag_c: cpu.regs.get_flag(CARRY_FLAG),
        }
    }
}

fn peek_byte(bus: &crate::hardware::bus::Bus, addr: u16) -> u8 {
    match addr {
        0x0000..=0x1FFF => bus.ram[(addr & 0x07FF) as usize],
        0x4020..=0xFFFF => bus.cartridge.cpu_read(addr),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_breakpoint() {
        let mut dc = DebugController::new();
        dc.add_breakpoint(0x8000);
        assert!(dc.has_breakpoint(0x8000));
        dc.remove_breakpoint(0x8000);
        assert!(!dc.has_breakpoint(0x8000));
    }

    #[test]
    fn toggle_breakpoint_adds_and_removes() {
        let mut dc = DebugController::new();
        dc.toggle_breakpoint(0xC000);
        assert!(dc.has_breakpoint(0xC000));
        dc.toggle_breakpoint(0xC000);
        assert!(!dc.has_breakpoint(0xC000));
    }

    #[test]
    fn should_break_at_breakpoint() {
        let mut dc = DebugController::new();
        dc.add_breakpoint(0x8000);
        assert!(dc.should_break(0x8000));
        assert_eq!(dc.hit_breakpoint, Some(0x8000));
    }

    #[test]
    fn should_not_break_without_breakpoints() {
        let mut dc = DebugController::new();
        assert!(!dc.should_break(0x8000));
    }

    #[test]
    fn break_on_next_fires_once() {
        let mut dc = DebugController::new();
        dc.break_on_next = true;
        assert!(dc.should_break(0x8000));
        assert!(!dc.should_break(0x8001));
    }

    #[test]
    fn watchpoint_write_detects_change() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0x0000, WatchType::Write);
        dc.check_watch_write(0x0000, 0x00, 0x42);
        let hit = dc.hit_watchpoint.unwrap();
        assert_eq!(hit.address, 0x0000);
        assert_eq!(hit.old_value, 0x00);
        assert_eq!(hit.new_value, 0x42);
    }

    #[test]
    fn watchpoint_write_ignores_same_value() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0x0000, WatchType::Write);
        dc.check_watch_write(0x0000, 0x42, 0x42);
        assert!(dc.hit_watchpoint.is_none());
    }

    #[test]
    fn watchpoint_read_fires() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0x0000, WatchType::Read);
        dc.check_watch_read(0x0000, 0x55);
        let hit = dc.hit_watchpoint.unwrap();
        assert_eq!(hit.address, 0x0000);
    }

    #[test]
    fn duplicate_watchpoint_not_added() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0x0000, WatchType::Write);
        dc.add_watchpoint(0x0000, WatchType::Write);
        assert_eq!(dc.watchpoints.len(), 1);
    }

    #[test]
    fn clear_hits_resets_state() {
        let mut dc = DebugController::new();
        dc.add_breakpoint(0x8000);
        dc.should_break(0x8000);
        dc.clear_hits();
        assert!(dc.hit_breakpoint.is_none());
        assert!(dc.hit_watchpoint.is_none());
    }
}

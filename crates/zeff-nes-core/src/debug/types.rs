use crate::hardware::cpu::registers::StatusFlags;

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
            p: cpu.regs.p.bits(),
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

            recent_ops: emu.opcode_log.recent(32),

            flag_n: cpu.regs.get_flag(StatusFlags::NEGATIVE),
            flag_v: cpu.regs.get_flag(StatusFlags::OVERFLOW),
            flag_d: cpu.regs.get_flag(StatusFlags::DECIMAL),
            flag_i: cpu.regs.get_flag(StatusFlags::INTERRUPT),
            flag_z: cpu.regs.get_flag(StatusFlags::ZERO),
            flag_c: cpu.regs.get_flag(StatusFlags::CARRY),
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

mod alu;
mod bitops;
mod registers;

use crate::hardware::bus::Bus;
use crate::hardware::opcodes::cycles::CYCLE_TABLE;
use crate::hardware::opcodes::dispatch::execute_opcode;
use crate::hardware::types::hardware_constants::*;
use crate::hardware::types::CPUState;
use crate::hardware::types::IMEState;

pub(crate) struct CPU {
    pub(crate) pc: u16,
    pub(crate) sp: u16,
    pub(crate) a: u8,
    pub(crate) f: u8,
    pub(crate) b: u8,
    pub(crate) c: u8,
    pub(crate) d: u8,
    pub(crate) e: u8,
    pub(crate) h: u8,
    pub(crate) l: u8,
    pub(crate) ime: IMEState,
    pub(crate) running: CPUState,
    pub(crate) cycles: u64,
    pub(crate) last_step_cycles: u64,
}

impl CPU {
    pub(crate) fn new() -> Self {
        Self {
            pc: 0x100,
            sp: 0xFFFE,
            a: 0x01,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            ime: IMEState::Disabled,
            running: CPUState::Running,
            cycles: 0,
            last_step_cycles: 0,
        }
    }

    pub(crate) fn step(&mut self, bus: &mut Bus) {
        if self.handle_interrupts(bus) {
            self.cycles += self.last_step_cycles;
            return;
        }

        if self.running == CPUState::Halted {
            // Wake from HALT if any interrupt is pending (IE & IF), even with IME disabled.
            let pending = bus.if_reg & bus.ie;
            if pending != 0 {
                self.running = CPUState::Running;
            } else {
                self.last_step_cycles = 4;
                self.cycles += self.last_step_cycles;
                return;
            }
        }

        let was_pending = self.ime == IMEState::PendingEnable;

        let opcode = self.fetch8(bus);
        self.last_step_cycles = CYCLE_TABLE[opcode as usize] as u64;
        execute_opcode(self, bus, opcode);
        self.cycles += self.last_step_cycles;

        if was_pending {
            self.ime = IMEState::Enabled;
        }
    }

    pub(crate) fn handle_interrupts(&mut self, bus: &mut Bus) -> bool {
        let triggered = bus.if_reg & bus.ie;
        if triggered == 0 || self.ime != IMEState::Enabled {
            return false;
        }

        if self.running == CPUState::Halted {
            self.running = CPUState::Running;
        }

        for bit in 0..5 {
            if triggered & (1 << bit) != 0 {
                self.ime = IMEState::Disabled;
                bus.if_reg &= !(1 << bit);

                self.push16(bus, self.pc);
                self.pc = match bit {
                    0 => INT_VBLANK,
                    1 => INT_STAT,
                    2 => INT_TIMER,
                    3 => INT_SERIAL,
                    4 => INT_JOYPAD,
                    _ => unreachable!(),
                };

                self.last_step_cycles = 20;
                return true;
            }
        }

        false
    }

    pub(crate) fn fetch8(&mut self, bus: &mut Bus) -> u8 {
        let val = bus.read_byte(self.pc);
        self.pc = self.pc.wrapping_add(1);
        val
    }

    pub(crate) fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let low = self.fetch8(bus) as u16;
        let high = self.fetch8(bus) as u16;
        low | (high << 8)
    }

    pub(crate) fn push16(&mut self, bus: &mut Bus, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write_byte(self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write_byte(self.sp, (value & 0xFF) as u8);
    }

    pub(crate) fn pop16(&mut self, bus: &mut Bus) -> u16 {
        let low = bus.read_byte(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let high = bus.read_byte(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (high << 8) | low
    }

    pub(crate) fn jump(&mut self, addr: u16) {
        self.pc = addr;
    }

    pub(crate) fn jump_relative(&mut self, offset: i8) {
        self.pc = self.pc.wrapping_add_signed(offset as i16);
    }
}
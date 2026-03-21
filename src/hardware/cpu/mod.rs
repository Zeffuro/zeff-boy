mod alu;
mod bitops;
mod registers;

use crate::hardware::bus::Bus;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::hardware::opcodes::cycles::CYCLE_TABLE;
use crate::hardware::opcodes::dispatch::execute_opcode;
use crate::hardware::types::constants::*;
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
    pub(crate) timed_cycles_accounted: u64,
    pub(crate) halt_bug_active: bool,
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
            timed_cycles_accounted: 0,
            halt_bug_active: false,
        }
    }

    pub(crate) fn step(&mut self, bus: &mut Bus) {
        self.timed_cycles_accounted = 0;
        let pending = bus.if_reg & bus.ie & 0x1F;

        if self.running == CPUState::Halted {
            if pending == 0 {
                self.tick_internal_timed(bus, 4);
                self.last_step_cycles = self.timed_cycles_accounted;
                self.cycles += self.last_step_cycles;
                return;
            }

            self.running = CPUState::Running;
            if self.ime == IMEState::Enabled {
                self.tick_internal_timed(bus, 4);
                if self.handle_interrupts(bus) {
                    self.last_step_cycles = self.timed_cycles_accounted;
                    self.cycles += self.last_step_cycles;
                    return;
                }
            } else {
                self.tick_internal_timed(bus, 4);
            }
        } else if self.ime == IMEState::Enabled && pending != 0 {
            if self.handle_interrupts(bus) {
                self.last_step_cycles = self.timed_cycles_accounted;
                self.cycles += self.last_step_cycles;
                return;
            }
        }

        let ime_was_pending_enable = matches!(self.ime, IMEState::PendingEnable);
        let opcode = self.fetch8_timed(bus);
        execute_opcode(self, bus, opcode);

        let expected_cycles = CYCLE_TABLE[opcode as usize] as u64;
        if self.timed_cycles_accounted < expected_cycles {
            self.tick_internal_timed(bus, expected_cycles - self.timed_cycles_accounted);
        }

        self.last_step_cycles = self.timed_cycles_accounted;
        self.cycles += self.last_step_cycles;

        if ime_was_pending_enable && matches!(self.ime, IMEState::PendingEnable) {
            self.ime = IMEState::Enabled;
        }
    }

    pub(crate) fn handle_interrupts(&mut self, bus: &mut Bus) -> bool {
        let triggered = bus.if_reg & bus.ie;
        if triggered == 0 || self.ime != IMEState::Enabled {
            return false;
        }

        for bit in 0..5 {
            if triggered & (1 << bit) != 0 {
                let irq_mask = 1 << bit;
                bus.if_reg &= !irq_mask;
                self.ime = IMEState::Disabled;

                self.tick_internal_timed(bus, 8);
                self.push16_timed(bus, self.pc);
                self.tick_internal_timed(bus, 4);
                self.pc = match bit {
                    0 => INT_VBLANK,
                    1 => INT_STAT,
                    2 => INT_TIMER,
                    3 => INT_SERIAL,
                    4 => INT_JOYPAD,
                    _ => unreachable!(),
                };

                return true;
            }
        }

        false
    }

    pub(crate) fn fetch8(&mut self, bus: &mut Bus) -> u8 {
        let val = bus.cpu_read_byte(self.pc);
        self.advance_pc_after_fetch();
        val
    }

    pub(crate) fn fetch8_timed(&mut self, bus: &mut Bus) -> u8 {
        let val = self.bus_read_timed(bus, self.pc);
        self.advance_pc_after_fetch();
        val
    }

    pub(crate) fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let low = self.fetch8(bus) as u16;
        let high = self.fetch8(bus) as u16;
        low | (high << 8)
    }

    pub(crate) fn fetch16_timed(&mut self, bus: &mut Bus) -> u16 {
        let low = self.fetch8_timed(bus) as u16;
        let high = self.fetch8_timed(bus) as u16;
        low | (high << 8)
    }

    #[allow(dead_code)]
    pub(crate) fn push16(&mut self, bus: &mut Bus, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.cpu_write_byte(self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.cpu_write_byte(self.sp, (value & 0xFF) as u8);
    }

    pub(crate) fn push16_timed(&mut self, bus: &mut Bus, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value & 0xFF) as u8);
    }

    #[allow(dead_code)]
    pub(crate) fn pop16(&mut self, bus: &mut Bus) -> u16 {
        let low = bus.cpu_read_byte(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let high = bus.cpu_read_byte(self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (high << 8) | low
    }

    pub(crate) fn pop16_timed(&mut self, bus: &mut Bus) -> u16 {
        let low = self.bus_read_timed(bus, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        let high = self.bus_read_timed(bus, self.sp) as u16;
        self.sp = self.sp.wrapping_add(1);
        (high << 8) | low
    }

    pub(crate) fn jump(&mut self, addr: u16) {
        self.pc = addr;
    }

    pub(crate) fn jump_relative(&mut self, offset: i8) {
        self.pc = self.pc.wrapping_add_signed(offset as i16);
    }

    pub(crate) fn bus_read_timed(&mut self, bus: &mut Bus, addr: u16) -> u8 {
        self.tick_peripherals(bus, 4);
        bus.cpu_read_byte(addr)
    }

    pub(crate) fn bus_write_timed(&mut self, bus: &mut Bus, addr: u16, value: u8) {
        self.tick_peripherals(bus, 4);
        let extra_t_cycles = bus.cpu_write_byte(addr, value);
        if extra_t_cycles != 0 {
            self.tick_peripherals(bus, extra_t_cycles);
        }
    }

    pub(crate) fn tick_internal_timed(&mut self, bus: &mut Bus, t_cycles: u64) {
        self.tick_peripherals(bus, t_cycles);
    }

    pub(crate) fn trigger_halt_bug(&mut self) {
        self.halt_bug_active = true;
    }

    fn tick_peripherals(&mut self, bus: &mut Bus, t_cycles: u64) {
        self.timed_cycles_accounted = self.timed_cycles_accounted.wrapping_add(t_cycles);

        if bus.io.timer.step(t_cycles) {
            bus.if_reg |= 0x04;
        }
        if bus.io.serial.step(t_cycles) {
            bus.if_reg |= 0x08;
        }
        bus.io.apu.step(t_cycles);

        let cgb_mode = matches!(bus.hardware_mode, HardwareMode::CGBNormal | HardwareMode::CGBDouble);
        let previous_ppu_mode = bus.io.ppu.mode();
        let ppu_interrupt = bus.io.ppu.step(t_cycles, &bus.vram, &bus.oam, cgb_mode);
        bus.if_reg |= ppu_interrupt;
        let current_ppu_mode = bus.io.ppu.mode();
        bus.maybe_step_hblank_hdma(previous_ppu_mode, current_ppu_mode);
        bus.step_oam_dma(t_cycles);
    }

    fn advance_pc_after_fetch(&mut self) {
        if self.halt_bug_active {
            self.halt_bug_active = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::rom_header::RomHeader;

    fn make_test_bus(mode: HardwareMode) -> Box<Bus> {
        let mut rom = vec![0u8; 0x8000];
        // Serial interrupt vector handler: JP $DEC3
        rom[0x0058] = 0xC3;
        rom[0x0059] = 0xC3;
        rom[0x005A] = 0xDE;
        let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
        Bus::new(rom, &header, mode).expect("test bus should initialize")
    }

    #[test]
    fn halt_bug_skips_next_pc_increment_once() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        bus.write_byte(0xC000, 0x00);

        cpu.trigger_halt_bug();

        let first = cpu.fetch8_timed(&mut bus);
        assert_eq!(first, 0x00);
        assert_eq!(cpu.pc, 0xC000);

        let second = cpu.fetch8_timed(&mut bus);
        assert_eq!(second, 0x00);
        assert_eq!(cpu.pc, 0xC001);
    }

    #[test]
    fn halted_with_ime_enabled_dispatches_interrupt_in_24_t_cycles() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC123;
        cpu.sp = 0xFFFE;
        cpu.running = CPUState::Halted;
        cpu.ime = IMEState::Enabled;
        bus.ie = 0x01;
        bus.if_reg = 0x01;

        cpu.step(&mut bus);

        assert_eq!(cpu.last_step_cycles, 24);
        assert_eq!(cpu.pc, INT_VBLANK);
        assert_eq!(cpu.sp, 0xFFFC);
        assert_eq!(bus.if_reg & 0x01, 0x00);
    }

    #[test]
    fn running_with_ime_enabled_dispatches_interrupt_in_20_t_cycles() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC123;
        cpu.sp = 0xFFFE;
        cpu.running = CPUState::Running;
        cpu.ime = IMEState::Enabled;
        bus.ie = 0x01;
        bus.if_reg = 0x01;

        cpu.step(&mut bus);

        assert_eq!(cpu.last_step_cycles, 20);
        assert_eq!(cpu.pc, INT_VBLANK);
        assert_eq!(cpu.sp, 0xFFFC);
        assert_eq!(bus.if_reg & 0x01, 0x00);
    }

    #[test]
    fn halted_with_ime_disabled_wakes_without_dispatch_and_executes_next_opcode() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.running = CPUState::Halted;
        cpu.ime = IMEState::Disabled;
        bus.ie = 0x01;
        bus.if_reg = 0x01;
        bus.write_byte(0xC000, 0x00);

        cpu.step(&mut bus);

        assert_eq!(cpu.last_step_cycles, 8);
        assert_eq!(cpu.pc, 0xC001);
        assert!(matches!(cpu.running, CPUState::Running));
        assert_eq!(bus.if_reg & 0x01, 0x01);
    }

    #[test]
    fn halted_with_ime_pending_enable_wakes_without_dispatch_and_enables_ime_after_instruction() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.running = CPUState::Halted;
        cpu.ime = IMEState::PendingEnable;
        bus.ie = 0x01;
        bus.if_reg = 0x01;
        bus.write_byte(0xC000, 0x00);

        cpu.step(&mut bus);

        assert_eq!(cpu.last_step_cycles, 8);
        assert_eq!(cpu.pc, 0xC001);
        assert!(matches!(cpu.running, CPUState::Running));
        assert!(matches!(cpu.ime, IMEState::Enabled));
        assert_eq!(bus.if_reg & 0x01, 0x01);
    }

    #[test]
    fn serial_interrupt_dispatch_plus_handler_is_13_m_cycles_in_dmg() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.sp = 0xFFFE;
        cpu.ime = IMEState::Enabled;
        bus.ie = 0x08;
        bus.if_reg = 0x08;

        bus.write_byte(0xDEC3, 0xC9); // RET at handler target

        let start_cycles = cpu.cycles;
        cpu.step(&mut bus); // interrupt dispatch: 5 M-cycles
        cpu.step(&mut bus); // JP $DEC3: 4 M-cycles
        cpu.step(&mut bus); // RET: 4 M-cycles

        assert_eq!(cpu.cycles - start_cycles, 13 * 4);
        assert_eq!(cpu.pc, 0xC000);
        assert_eq!(bus.if_reg & 0x08, 0x00);
    }

    #[test]
    fn serial_interrupt_dispatch_plus_handler_is_13_m_cycles_in_cgb_double() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::CGBDouble);
        cpu.pc = 0xC000;
        cpu.sp = 0xFFFE;
        cpu.ime = IMEState::Enabled;
        bus.ie = 0x08;
        bus.if_reg = 0x08;

        bus.write_byte(0xDEC3, 0xC9); // RET at handler target

        let start_cycles = cpu.cycles;
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        cpu.step(&mut bus);

        assert_eq!(cpu.cycles - start_cycles, 13 * 4);
        assert_eq!(cpu.pc, 0xC000);
        assert_eq!(bus.if_reg & 0x08, 0x00);
    }

    #[test]
    fn gdma_write_consumes_block_cycles_in_cgb_normal() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::CGBNormal);

        for i in 0..0x10u16 {
            bus.write_byte(0xC000 + i, (0x80 + i as u8) as u8);
        }

        bus.write_byte(CGB_HDMA1, 0xC0);
        bus.write_byte(CGB_HDMA2, 0x00);
        bus.write_byte(CGB_HDMA3, 0x80);
        bus.write_byte(CGB_HDMA4, 0x00);

        let before = cpu.timed_cycles_accounted;
        cpu.bus_write_timed(&mut bus, CGB_HDMA5, 0x00);
        let delta = cpu.timed_cycles_accounted - before;

        assert_eq!(delta, 4 + 32);
        assert!(!bus.hdma_active);
        assert_eq!(&bus.vram[0..0x10], &[0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B, 0x8C, 0x8D, 0x8E, 0x8F]);
    }

    #[test]
    fn gdma_write_consumes_block_cycles_in_cgb_double() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::CGBDouble);

        for i in 0..0x10u16 {
            bus.write_byte(0xC000 + i, (0x40 + i as u8) as u8);
        }

        bus.write_byte(CGB_HDMA1, 0xC0);
        bus.write_byte(CGB_HDMA2, 0x00);
        bus.write_byte(CGB_HDMA3, 0x80);
        bus.write_byte(CGB_HDMA4, 0x00);

        let before = cpu.timed_cycles_accounted;
        cpu.bus_write_timed(&mut bus, CGB_HDMA5, 0x00);
        let delta = cpu.timed_cycles_accounted - before;

        assert_eq!(delta, 4 + 64);
        assert!(!bus.hdma_active);
        assert_eq!(&bus.vram[0..0x10], &[0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F]);
    }
}

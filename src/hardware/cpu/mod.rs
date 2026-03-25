mod alu;
mod bitops;
mod registers;

pub(crate) use registers::Registers;

use crate::hardware::bus::Bus;
use crate::hardware::opcodes::cycles::CYCLE_TABLE;
use crate::hardware::opcodes::dispatch::execute_opcode;
use crate::hardware::types::CPUState;
use crate::hardware::types::IMEState;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

#[derive(Debug)]
pub(crate) struct CPU {
    pub(crate) pc: u16,
    pub(crate) sp: u16,
    pub(crate) regs: Registers,
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
            regs: Registers::default(),
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
                self.commit_step_cycles();
                return;
            }

            self.running = CPUState::Running;
            if self.ime == IMEState::Enabled {
                self.tick_internal_timed(bus, 4);
                if self.handle_interrupts(bus) {
                    self.commit_step_cycles();
                    return;
                }
            } else {
                self.tick_internal_timed(bus, 4);
            }
        } else if self.ime == IMEState::Enabled && pending != 0
            && self.handle_interrupts(bus) {
                self.commit_step_cycles();
                return;
            }

        let ime_was_pending_enable = matches!(self.ime, IMEState::PendingEnable);
        let opcode = self.fetch8_timed(bus);
        execute_opcode(self, bus, opcode);

        let expected_cycles = CYCLE_TABLE[opcode as usize] as u64;
        if self.timed_cycles_accounted < expected_cycles {
            self.tick_internal_timed(bus, expected_cycles - self.timed_cycles_accounted);
        }

        self.commit_step_cycles();

        if ime_was_pending_enable && matches!(self.ime, IMEState::PendingEnable) {
            self.ime = IMEState::Enabled;
        }
    }

    fn commit_step_cycles(&mut self) {
        self.last_step_cycles = self.timed_cycles_accounted;
        self.cycles += self.last_step_cycles;
    }

    pub(crate) fn handle_interrupts(&mut self, bus: &mut Bus) -> bool {
        let triggered = bus.if_reg & bus.ie;
        if triggered == 0 || self.ime != IMEState::Enabled {
            return false;
        }

        const INT_VECTORS: [u16; 5] = [INT_VBLANK, INT_STAT, INT_TIMER, INT_SERIAL, INT_JOYPAD];

        let bit = (triggered & 0x1F).trailing_zeros() as usize;
        if bit >= 5 {
            return false;
        }

        bus.if_reg &= !(1 << bit);
        self.ime = IMEState::Disabled;

        self.tick_internal_timed(bus, 8);
        self.push16_timed(bus, self.pc);
        self.tick_internal_timed(bus, 4);
        self.pc = INT_VECTORS[bit];

        true
    }

    pub(crate) fn fetch8_timed(&mut self, bus: &mut Bus) -> u8 {
        let val = self.bus_read_timed(bus, self.pc);
        self.advance_pc_after_fetch();
        val
    }


    pub(crate) fn fetch16_timed(&mut self, bus: &mut Bus) -> u16 {
        let low = self.fetch8_timed(bus) as u16;
        let high = self.fetch8_timed(bus) as u16;
        low | (high << 8)
    }


    pub(crate) fn push16_timed(&mut self, bus: &mut Bus, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        self.bus_write_timed(bus, self.sp, (value & 0xFF) as u8);
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

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u16(self.pc);
        writer.write_u16(self.sp);
        writer.write_u8(self.regs.a);
        writer.write_u8(self.regs.f);
        writer.write_u8(self.regs.b);
        writer.write_u8(self.regs.c);
        writer.write_u8(self.regs.d);
        writer.write_u8(self.regs.e);
        writer.write_u8(self.regs.h);
        writer.write_u8(self.regs.l);
        writer.write_u8(encode_ime_state(self.ime));
        writer.write_u8(encode_cpu_state(self.running));
        writer.write_u64(self.cycles);
        writer.write_u64(self.last_step_cycles);
        writer.write_u64(self.timed_cycles_accounted);
        writer.write_bool(self.halt_bug_active);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            pc: reader.read_u16()?,
            sp: reader.read_u16()?,
            regs: Registers {
                a: reader.read_u8()?,
                f: reader.read_u8()?,
                b: reader.read_u8()?,
                c: reader.read_u8()?,
                d: reader.read_u8()?,
                e: reader.read_u8()?,
                h: reader.read_u8()?,
                l: reader.read_u8()?,
            },
            ime: decode_ime_state(reader.read_u8()?)?,
            running: decode_cpu_state(reader.read_u8()?)?,
            cycles: reader.read_u64()?,
            last_step_cycles: reader.read_u64()?,
            timed_cycles_accounted: reader.read_u64()?,
            halt_bug_active: reader.read_bool()?,
        })
    }

    fn tick_peripherals(&mut self, bus: &mut Bus, t_cycles: u64) {
        self.timed_cycles_accounted = self.timed_cycles_accounted.wrapping_add(t_cycles);

        let is_double_speed = bus.hardware_mode == HardwareMode::CGBDouble;
        let system_t_cycles = if is_double_speed {
            t_cycles / 2
        } else {
            t_cycles
        };

        bus.step_timer(t_cycles);
        bus.step_serial(t_cycles);
        bus.step_apu(system_t_cycles);

        let previous_ppu_mode = bus.ppu_mode();
        let ppu_interrupt = bus.step_ppu(system_t_cycles);
        bus.if_reg |= ppu_interrupt;

        let current_ppu_mode = bus.ppu_mode();
        bus.maybe_step_hblank_hdma(previous_ppu_mode, current_ppu_mode);

        bus.step_oam_dma(t_cycles);

        bus.cartridge.step(system_t_cycles);
    }

    fn advance_pc_after_fetch(&mut self) {
        if self.halt_bug_active {
            self.halt_bug_active = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
    }
}

fn encode_ime_state(state: IMEState) -> u8 {
    match state {
        IMEState::Enabled => 0,
        IMEState::Disabled => 1,
        IMEState::PendingEnable => 2,
    }
}

fn decode_ime_state(tag: u8) -> Result<IMEState> {
    match tag {
        0 => Ok(IMEState::Enabled),
        1 => Ok(IMEState::Disabled),
        2 => Ok(IMEState::PendingEnable),
        _ => bail!("invalid IME state tag in save-state file: {tag}"),
    }
}

fn encode_cpu_state(state: CPUState) -> u8 {
    match state {
        CPUState::Running => 0,
        CPUState::Halted => 1,
        CPUState::Stopped => 2,
        CPUState::InterruptHandling => 3,
        CPUState::Reset => 4,
        CPUState::Suspended => 5,
    }
}

fn decode_cpu_state(tag: u8) -> Result<CPUState> {
    match tag {
        0 => Ok(CPUState::Running),
        1 => Ok(CPUState::Halted),
        2 => Ok(CPUState::Stopped),
        3 => Ok(CPUState::InterruptHandling),
        4 => Ok(CPUState::Reset),
        5 => Ok(CPUState::Suspended),
        _ => bail!("invalid CPU state tag in save-state file: {tag}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::rom_header::RomHeader;

    fn make_test_bus(mode: HardwareMode) -> Bus {
        let mut rom = vec![0u8; 0x8000];
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

        bus.write_byte(0xDEC3, 0xC9);

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
            bus.write_byte(0xC000 + i, 0x80 + i as u8);
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
    }

    #[test]
    fn af_round_trips_with_lower_nibble_cleared() {
        let mut cpu = CPU::new();
        cpu.set_af(0x12F0);
        assert_eq!(cpu.regs.a, 0x12);
        assert_eq!(cpu.regs.f, 0xF0);
        assert_eq!(cpu.get_af(), 0x12F0);

        cpu.set_af(0xABCD);
        assert_eq!(cpu.regs.f, 0xC0);
        assert_eq!(cpu.get_af(), 0xABC0);
    }

    #[test]
    fn bc_de_hl_round_trip() {
        let mut cpu = CPU::new();
        cpu.set_bc(0x1234);
        assert_eq!(cpu.regs.b, 0x12);
        assert_eq!(cpu.regs.c, 0x34);
        assert_eq!(cpu.get_bc(), 0x1234);

        cpu.set_de(0x5678);
        assert_eq!(cpu.regs.d, 0x56);
        assert_eq!(cpu.regs.e, 0x78);
        assert_eq!(cpu.get_de(), 0x5678);

        cpu.set_hl(0x9ABC);
        assert_eq!(cpu.regs.h, 0x9A);
        assert_eq!(cpu.regs.l, 0xBC);
        assert_eq!(cpu.get_hl(), 0x9ABC);
    }

    #[test]
    fn flag_getters_and_setters() {
        let mut cpu = CPU::new();
        cpu.regs.f = 0x00;
        assert!(!cpu.get_z());
        assert!(!cpu.get_n());
        assert!(!cpu.get_h());
        assert!(!cpu.get_c());

        cpu.set_z(true);
        assert!(cpu.get_z());
        assert_eq!(cpu.regs.f & 0x80, 0x80);

        cpu.set_n(true);
        assert!(cpu.get_n());

        cpu.set_h(true);
        assert!(cpu.get_h());

        cpu.set_c(true);
        assert!(cpu.get_c());
        assert_eq!(cpu.regs.f, 0xF0);

        cpu.set_z(false);
        assert!(!cpu.get_z());
        assert_eq!(cpu.regs.f, 0x70);
    }

    #[test]
    fn add_zero_plus_zero_sets_zero_flag() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0;
        cpu.add(0);
        assert_eq!(cpu.regs.a, 0);
        assert!(cpu.get_z());
        assert!(!cpu.get_n());
        assert!(!cpu.get_h());
        assert!(!cpu.get_c());
    }

    #[test]
    fn add_half_carry() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x0F;
        cpu.add(0x01);
        assert_eq!(cpu.regs.a, 0x10);
        assert!(!cpu.get_z());
        assert!(cpu.get_h());
        assert!(!cpu.get_c());
    }

    #[test]
    fn add_full_carry_and_wrap() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0xFF;
        cpu.add(0x01);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.get_z());
        assert!(cpu.get_h());
        assert!(cpu.get_c());
    }

    #[test]
    fn sub_equal_values_gives_zero() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x42;
        cpu.sub(0x42);
        assert_eq!(cpu.regs.a, 0);
        assert!(cpu.get_z());
        assert!(cpu.get_n());
        assert!(!cpu.get_h());
        assert!(!cpu.get_c());
    }

    #[test]
    fn sub_half_borrow() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x10;
        cpu.sub(0x01);
        assert_eq!(cpu.regs.a, 0x0F);
        assert!(cpu.get_h());
        assert!(!cpu.get_c());
    }

    #[test]
    fn sub_full_borrow_wraps() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x00;
        cpu.sub(0x01);
        assert_eq!(cpu.regs.a, 0xFF);
        assert!(cpu.get_c());
        assert!(cpu.get_n());
    }

    #[test]
    fn adc_with_carry_set() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x0E;
        cpu.set_c(true);
        cpu.adc(0x01);
        assert_eq!(cpu.regs.a, 0x10);
        assert!(cpu.get_h());
        assert!(!cpu.get_c());
    }

    #[test]
    fn adc_wraps_with_carry() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0xFF;
        cpu.set_c(true);
        cpu.adc(0x00);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.get_z());
        assert!(cpu.get_c());
    }

    #[test]
    fn sbc_with_carry_set() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x10;
        cpu.set_c(true);
        cpu.sbc(0x0F);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.get_z());
        assert!(cpu.get_n());
    }

    #[test]
    fn sbc_borrow_propagation() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x00;
        cpu.set_c(true);
        cpu.sbc(0x00);
        assert_eq!(cpu.regs.a, 0xFF);
        assert!(cpu.get_c());
        assert!(cpu.get_h());
    }

    #[test]
    fn inc_wraps_and_sets_zero() {
        let mut cpu = CPU::new();
        cpu.set_c(true);
        let r = cpu.inc(0xFF);
        assert_eq!(r, 0x00);
        assert!(cpu.get_z());
        assert!(!cpu.get_n());
        assert!(cpu.get_h());
        assert!(cpu.get_c());
    }

    #[test]
    fn dec_wraps_and_sets_half_carry() {
        let mut cpu = CPU::new();
        let r = cpu.dec(0x00);
        assert_eq!(r, 0xFF);
        assert!(!cpu.get_z());
        assert!(cpu.get_n());
        assert!(cpu.get_h());
    }

    #[test]
    fn inc_half_carry_boundary() {
        let mut cpu = CPU::new();
        let r = cpu.inc(0x0F);
        assert_eq!(r, 0x10);
        assert!(cpu.get_h());
    }

    #[test]
    fn logical_and_sets_half_carry() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0xFF;
        cpu.logical_and(0x0F);
        assert_eq!(cpu.regs.a, 0x0F);
        assert!(!cpu.get_z());
        assert!(cpu.get_h());
        assert!(!cpu.get_c());
    }

    #[test]
    fn logical_or_zero_result() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x00;
        cpu.logical_or(0x00);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.get_z());
        assert!(!cpu.get_h());
    }

    #[test]
    fn logical_xor_self_gives_zero() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0xAB;
        cpu.logical_xor(0xAB);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.get_z());
    }

    #[test]
    fn compare_sets_flags_without_modifying_a() {
        let mut cpu = CPU::new();
        cpu.regs.a = 0x42;
        cpu.compare(0x42);
        assert_eq!(cpu.regs.a, 0x42);
        assert!(cpu.get_z());
        assert!(cpu.get_n());
    }

    #[test]
    fn rlc_rotates_and_sets_carry() {
        let mut cpu = CPU::new();
        let r = cpu.rlc(0x80);
        assert_eq!(r, 0x01);
        assert!(cpu.get_c());
        assert!(!cpu.get_z());
    }

    #[test]
    fn rlc_zero() {
        let mut cpu = CPU::new();
        let r = cpu.rlc(0x00);
        assert_eq!(r, 0x00);
        assert!(cpu.get_z());
        assert!(!cpu.get_c());
    }

    #[test]
    fn rrc_rotates_and_sets_carry() {
        let mut cpu = CPU::new();
        let r = cpu.rrc(0x01);
        assert_eq!(r, 0x80);
        assert!(cpu.get_c());
    }

    #[test]
    fn rl_through_carry() {
        let mut cpu = CPU::new();
        cpu.set_c(true);
        let r = cpu.rl(0x80);
        assert_eq!(r, 0x01);
        assert!(cpu.get_c());
    }

    #[test]
    fn rr_through_carry() {
        let mut cpu = CPU::new();
        cpu.set_c(true);
        let r = cpu.rr(0x01);
        assert_eq!(r, 0x80);
        assert!(cpu.get_c());
    }

    #[test]
    fn sla_shifts_left_and_clears_bit0() {
        let mut cpu = CPU::new();
        let r = cpu.sla(0x80);
        assert_eq!(r, 0x00);
        assert!(cpu.get_z());
        assert!(cpu.get_c());
    }

    #[test]
    fn srl_shifts_right_and_clears_bit7() {
        let mut cpu = CPU::new();
        let r = cpu.srl(0x01);
        assert_eq!(r, 0x00);
        assert!(cpu.get_z());
        assert!(cpu.get_c());
    }

    #[test]
    fn sra_preserves_sign_bit() {
        let mut cpu = CPU::new();
        let r = cpu.sra(0x80);
        assert_eq!(r, 0xC0);
        assert!(!cpu.get_c());
    }

    #[test]
    fn swap_nibbles() {
        let mut cpu = CPU::new();
        let r = cpu.swap(0xF0);
        assert_eq!(r, 0x0F);
        assert!(!cpu.get_z());
        assert!(!cpu.get_c());
    }

    #[test]
    fn swap_zero() {
        let mut cpu = CPU::new();
        let r = cpu.swap(0x00);
        assert_eq!(r, 0x00);
        assert!(cpu.get_z());
    }

    #[test]
    fn bit_test_set_and_clear() {
        let mut cpu = CPU::new();
        cpu.bit(0, 0x01);
        assert!(!cpu.get_z());
        assert!(cpu.get_h());

        cpu.bit(7, 0x01);
        assert!(cpu.get_z());
    }

    #[test]
    fn set_and_res_bits() {
        let mut cpu = CPU::new();
        let r = cpu.set(3, 0x00);
        assert_eq!(r, 0x08);

        let r = cpu.res(3, 0xFF);
        assert_eq!(r, 0xF7);
    }

    #[test]
    fn step_ld_b_d8_then_ld_a_b() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        bus.write_byte(0xC000, 0x06);
        bus.write_byte(0xC001, 0x42);
        bus.write_byte(0xC002, 0x78);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.b, 0x42);
        assert_eq!(cpu.pc, 0xC002);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x42);
        assert_eq!(cpu.pc, 0xC003);
    }

    #[test]
    fn step_call_and_ret_round_trip() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.sp = 0xFFFE;
        bus.write_byte(0xC000, 0xCD);
        bus.write_byte(0xC001, 0x10);
        bus.write_byte(0xC002, 0xC0);
        bus.write_byte(0xC010, 0xC9);

        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0xC010);
        assert_eq!(cpu.sp, 0xFFFC);

        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0xC003);
        assert_eq!(cpu.sp, 0xFFFE);
    }

    #[test]
    fn step_push_pop_preserves_register_pair() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.sp = 0xFFFE;
        cpu.set_bc(0xABCD);
        bus.write_byte(0xC000, 0xC5);
        bus.write_byte(0xC001, 0xD1);

        cpu.step(&mut bus);
        assert_eq!(cpu.sp, 0xFFFC);

        cpu.step(&mut bus);
        assert_eq!(cpu.sp, 0xFFFE);
        assert_eq!(cpu.get_de(), 0xABCD);
    }

    #[test]
    fn step_jr_nz_takes_branch_when_z_clear() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        bus.write_byte(0xC000, 0x3E);
        bus.write_byte(0xC001, 0x01);
        bus.write_byte(0xC002, 0xB7);
        bus.write_byte(0xC003, 0x20);
        bus.write_byte(0xC004, 0x03);

        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert!(!cpu.get_z());
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0xC008);
    }

    #[test]
    fn step_jr_nz_falls_through_when_z_set() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        bus.write_byte(0xC000, 0xAF);
        bus.write_byte(0xC001, 0x20);
        bus.write_byte(0xC002, 0x05);
        bus.write_byte(0xC003, 0x00);

        cpu.step(&mut bus);
        assert!(cpu.get_z());
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0xC003);
    }

    #[test]
    fn step_inc_dec_memory_via_hl() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        bus.write_byte(0xC100, 0x00);
        bus.write_byte(0xC000, 0x21);
        bus.write_byte(0xC001, 0x00);
        bus.write_byte(0xC002, 0xC1);
        bus.write_byte(0xC003, 0x34);
        bus.write_byte(0xC004, 0x34);
        bus.write_byte(0xC005, 0x35);

        cpu.step(&mut bus);
        assert_eq!(cpu.get_hl(), 0xC100);

        cpu.step(&mut bus);
        assert_eq!(bus.read_byte(0xC100), 0x01);

        cpu.step(&mut bus);
        assert_eq!(bus.read_byte(0xC100), 0x02);

        cpu.step(&mut bus);
        assert_eq!(bus.read_byte(0xC100), 0x01);
    }

    #[test]
    fn step_daa_corrects_bcd_addition() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        bus.write_byte(0xC000, 0x3E);
        bus.write_byte(0xC001, 0x45);
        bus.write_byte(0xC002, 0xC6);
        bus.write_byte(0xC003, 0x38);
        bus.write_byte(0xC004, 0x27);

        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x7D);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x83);
        assert!(!cpu.get_z());
    }

    #[test]
    fn step_ei_enables_ime_after_next_instruction() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.ime = IMEState::Disabled;
        bus.write_byte(0xC000, 0xFB);
        bus.write_byte(0xC001, 0x00);

        cpu.step(&mut bus);
        assert!(matches!(cpu.ime, IMEState::PendingEnable));

        cpu.step(&mut bus);
        assert!(matches!(cpu.ime, IMEState::Enabled));
    }

    #[test]
    fn step_di_disables_ime_immediately() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.ime = IMEState::Enabled;
        bus.write_byte(0xC000, 0xF3);

        cpu.step(&mut bus);
        assert!(matches!(cpu.ime, IMEState::Disabled));
    }

    #[test]
    fn step_ld_hl_sp_r8_positive_offset() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.sp = 0xFFF0;
        bus.write_byte(0xC000, 0xF8);
        bus.write_byte(0xC001, 0x05);

        cpu.step(&mut bus);
        assert_eq!(cpu.get_hl(), 0xFFF5);
        assert!(!cpu.get_z());
        assert!(!cpu.get_n());
    }

    #[test]
    fn step_ld_hl_sp_r8_negative_offset() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.sp = 0xFFF0;
        bus.write_byte(0xC000, 0xF8);
        bus.write_byte(0xC001, 0xFD);

        cpu.step(&mut bus);
        assert_eq!(cpu.get_hl(), 0xFFED);
    }

    #[test]
    fn step_cpl_complements_a() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        bus.write_byte(0xC000, 0x3E);
        bus.write_byte(0xC001, 0x35);
        bus.write_byte(0xC002, 0x2F);

        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0xCA);
        assert!(cpu.get_n());
        assert!(cpu.get_h());
    }

    #[test]
    fn step_scf_ccf_toggle_carry() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.set_c(false);
        bus.write_byte(0xC000, 0x37);
        bus.write_byte(0xC001, 0x3F);

        cpu.step(&mut bus);
        assert!(cpu.get_c());

        cpu.step(&mut bus);
        assert!(!cpu.get_c());
    }

    #[test]
    fn step_ld_a16_sp_stores_sp_in_memory() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.sp = 0xABCD;
        bus.write_byte(0xC000, 0x08);
        bus.write_byte(0xC001, 0x00);
        bus.write_byte(0xC002, 0xC1);

        cpu.step(&mut bus);
        assert_eq!(bus.read_byte(0xC100), 0xCD);
        assert_eq!(bus.read_byte(0xC101), 0xAB);
    }

    #[test]
    fn step_rst_pushes_pc_and_jumps() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.sp = 0xFFFE;
        bus.write_byte(0xC000, 0xFF);

        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x0038);
        assert_eq!(cpu.sp, 0xFFFC);

        let low = bus.read_byte(0xFFFC);
        let high = bus.read_byte(0xFFFD);
        assert_eq!((high as u16) << 8 | low as u16, 0xC001);
    }

    #[test]
    fn step_add_sp_r8_flags() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.sp = 0x00FF;
        bus.write_byte(0xC000, 0xE8);
        bus.write_byte(0xC001, 0x01);

        cpu.step(&mut bus);
        assert_eq!(cpu.sp, 0x0100);
        assert!(!cpu.get_z());
        assert!(!cpu.get_n());
        assert!(cpu.get_h());
        assert!(cpu.get_c());
    }

    #[test]
    fn step_cb_rlc_b_rotates_register() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.regs.b = 0x85;
        bus.write_byte(0xC000, 0xCB);
        bus.write_byte(0xC001, 0x00);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.b, 0x0B);
        assert!(cpu.get_c());
        assert!(!cpu.get_z());
    }

    #[test]
    fn step_cb_bit_7_a_tests_bit_without_modifying() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.regs.a = 0x80;
        bus.write_byte(0xC000, 0xCB);
        bus.write_byte(0xC001, 0x7F);

        cpu.step(&mut bus);
        assert!(!cpu.get_z());
        assert!(cpu.get_h());
        assert_eq!(cpu.regs.a, 0x80);
        cpu.pc = 0xC000;
        cpu.regs.a = 0x7F;
        cpu.step(&mut bus);
        assert!(cpu.get_z());
    }

    #[test]
    fn step_cb_swap_a_swaps_nibbles() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.regs.a = 0xAB;
        bus.write_byte(0xC000, 0xCB);
        bus.write_byte(0xC001, 0x37);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0xBA);
        assert!(!cpu.get_z());
        assert!(!cpu.get_c());
    }

    #[test]
    fn step_cb_set_and_res_bit_in_register() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.regs.b = 0x00;
        bus.write_byte(0xC000, 0xCB);
        bus.write_byte(0xC001, 0xF8);
        bus.write_byte(0xC002, 0xCB);
        bus.write_byte(0xC003, 0x88);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.b, 0x80);

        cpu.regs.b = 0xFF;
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.b, 0xFD);
    }

    #[test]
    fn step_cb_srl_a_shifts_right_logically() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.regs.a = 0x81;
        bus.write_byte(0xC000, 0xCB);
        bus.write_byte(0xC001, 0x3F);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x40);
        assert!(cpu.get_c());
        assert!(!cpu.get_z());
    }

    #[test]
    fn step_ldi_a_hl_loads_and_increments_hl() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.set_hl(0xC100);
        bus.write_byte(0xC100, 0x55);
        bus.write_byte(0xC000, 0x2A);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x55);
        assert_eq!(cpu.get_hl(), 0xC101);
    }

    #[test]
    fn step_ldd_a_hl_loads_and_decrements_hl() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.set_hl(0xC100);
        bus.write_byte(0xC100, 0x77);
        bus.write_byte(0xC000, 0x3A);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x77);
        assert_eq!(cpu.get_hl(), 0xC0FF);
    }

    #[test]
    fn step_ld_hl_plus_a_stores_and_increments_hl() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.regs.a = 0xAA;
        cpu.set_hl(0xC100);
        bus.write_byte(0xC000, 0x22);

        cpu.step(&mut bus);
        assert_eq!(bus.read_byte(0xC100), 0xAA);
        assert_eq!(cpu.get_hl(), 0xC101);
    }

    #[test]
    fn step_add_hl_bc_16bit_addition() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.set_hl(0x8A23);
        cpu.set_bc(0x0605);
        bus.write_byte(0xC000, 0x09);

        cpu.step(&mut bus);
        assert_eq!(cpu.get_hl(), 0x9028);
        assert!(!cpu.get_n());
        assert!(cpu.get_h());
        assert!(!cpu.get_c());
    }

    #[test]
    fn step_add_hl_hl_doubles_value() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.set_hl(0x8000);
        bus.write_byte(0xC000, 0x29);

        cpu.step(&mut bus);
        assert_eq!(cpu.get_hl(), 0x0000);
        assert!(cpu.get_c());
    }

    #[test]
    fn step_jp_nn_jumps_to_immediate_address() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        bus.write_byte(0xC000, 0xC3);
        bus.write_byte(0xC001, 0x50);
        bus.write_byte(0xC002, 0xC0);

        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0xC050);
    }

    #[test]
    fn step_xor_a_zeroes_a_and_sets_z() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.regs.a = 0xFF;
        bus.write_byte(0xC000, 0xAF);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.get_z());
        assert!(!cpu.get_n());
        assert!(!cpu.get_h());
        assert!(!cpu.get_c());
    }

    #[test]
    fn step_ld_sp_hl_copies_hl_to_sp() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.set_hl(0xDEAD);
        bus.write_byte(0xC000, 0xF9);

        cpu.step(&mut bus);
        assert_eq!(cpu.sp, 0xDEAD);
    }

    #[test]
    fn step_cp_d8_sets_flags_without_modifying_a() {
        let mut cpu = CPU::new();
        let mut bus = make_test_bus(HardwareMode::DMG);
        cpu.pc = 0xC000;
        cpu.regs.a = 0x42;
        bus.write_byte(0xC000, 0xFE);
        bus.write_byte(0xC001, 0x42);

        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x42);
        assert!(cpu.get_z());
        assert!(cpu.get_n());
    }
}

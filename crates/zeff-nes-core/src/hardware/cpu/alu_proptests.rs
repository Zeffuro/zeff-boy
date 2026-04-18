use proptest::prelude::*;

use super::Cpu;
use super::registers::StatusFlags;

fn test_cpu(a: u8, carry: bool) -> Cpu {
    let mut cpu = Cpu::new();
    cpu.regs.a = a;
    cpu.regs.set_flag(StatusFlags::CARRY, carry);
    cpu
}

proptest! {
    #[test]
    fn adc_result(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.adc(b);
        let sum = (a as u16) + (b as u16) + (carry as u16);
        prop_assert_eq!(cpu.regs.a, sum as u8);
    }

    #[test]
    fn adc_carry_flag(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.adc(b);
        let sum = (a as u16) + (b as u16) + (carry as u16);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::CARRY), sum > 0xFF);
    }

    #[test]
    fn adc_zero_flag(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.adc(b);
        let result = ((a as u16) + (b as u16) + (carry as u16)) as u8;
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::ZERO), result == 0);
    }

    #[test]
    fn adc_negative_flag(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.adc(b);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::NEGATIVE), cpu.regs.a & 0x80 != 0);
    }

    #[test]
    fn adc_overflow_flag(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.adc(b);
        let sum = (a as u16) + (b as u16) + (carry as u16);
        let overflow = (!(a as u16 ^ b as u16) & (a as u16 ^ sum)) & 0x80 != 0;
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::OVERFLOW), overflow);
    }

    #[test]
    fn sbc_via_adc_complement(a: u8, b: u8, carry: bool) {
        let mut cpu_sbc = test_cpu(a, carry);
        cpu_sbc.sbc(b);

        let mut cpu_adc = test_cpu(a, carry);
        cpu_adc.adc(!b);

        prop_assert_eq!(cpu_sbc.regs.a, cpu_adc.regs.a);
        prop_assert_eq!(
            cpu_sbc.regs.get_flag(StatusFlags::CARRY),
            cpu_adc.regs.get_flag(StatusFlags::CARRY)
        );
        prop_assert_eq!(
            cpu_sbc.regs.get_flag(StatusFlags::OVERFLOW),
            cpu_adc.regs.get_flag(StatusFlags::OVERFLOW)
        );
    }

    #[test]
    fn compare_does_not_modify_a(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.compare(a, b);
        prop_assert_eq!(cpu.regs.a, a);
    }

    #[test]
    fn compare_carry_flag(reg: u8, val: u8) {
        let mut cpu = test_cpu(0, false);
        cpu.compare(reg, val);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::CARRY), reg >= val);
    }

    #[test]
    fn compare_zero_flag(reg: u8, val: u8) {
        let mut cpu = test_cpu(0, false);
        cpu.compare(reg, val);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::ZERO), reg == val);
    }

    #[test]
    fn compare_negative_flag(reg: u8, val: u8) {
        let mut cpu = test_cpu(0, false);
        cpu.compare(reg, val);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::NEGATIVE), reg.wrapping_sub(val) & 0x80 != 0);
    }

    #[test]
    fn asl_val_result(val: u8) {
        let mut cpu = Cpu::new();
        let result = cpu.asl_val(val);
        prop_assert_eq!(result, val << 1);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::CARRY), val & 0x80 != 0);
    }

    #[test]
    fn lsr_val_result(val: u8) {
        let mut cpu = Cpu::new();
        let result = cpu.lsr_val(val);
        prop_assert_eq!(result, val >> 1);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::CARRY), val & 0x01 != 0);
    }

    #[test]
    fn rol_val_result(val: u8, carry: bool) {
        let mut cpu = test_cpu(0, carry);
        let result = cpu.rol_val(val);
        prop_assert_eq!(result, (val << 1) | carry as u8);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::CARRY), val & 0x80 != 0);
    }

    #[test]
    fn ror_val_result(val: u8, carry: bool) {
        let mut cpu = test_cpu(0, carry);
        let result = cpu.ror_val(val);
        let expected = (val >> 1) | if carry { 0x80 } else { 0 };
        prop_assert_eq!(result, expected);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::CARRY), val & 0x01 != 0);
    }

    #[test]
    fn inc_val_result(val: u8) {
        let mut cpu = Cpu::new();
        let result = cpu.inc_val(val);
        prop_assert_eq!(result, val.wrapping_add(1));
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::ZERO), result == 0);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::NEGATIVE), result & 0x80 != 0);
    }

    #[test]
    fn dec_val_result(val: u8) {
        let mut cpu = Cpu::new();
        let result = cpu.dec_val(val);
        prop_assert_eq!(result, val.wrapping_sub(1));
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::ZERO), result == 0);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::NEGATIVE), result & 0x80 != 0);
    }

    #[test]
    fn bit_test_zero_flag(a: u8, val: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.bit_test(val);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::ZERO), a & val == 0);
    }

    #[test]
    fn bit_test_overflow_and_negative(val: u8) {
        let mut cpu = test_cpu(0xFF, false);
        cpu.bit_test(val);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::OVERFLOW), val & 0x40 != 0);
        prop_assert_eq!(cpu.regs.get_flag(StatusFlags::NEGATIVE), val & 0x80 != 0);
    }
}

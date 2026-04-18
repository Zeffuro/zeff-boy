use proptest::prelude::*;

use super::Cpu;

fn test_cpu(a: u8, carry: bool) -> Cpu {
    let mut cpu = Cpu::new();
    cpu.regs.a = a;
    cpu.set_c(carry);
    cpu
}

proptest! {
    #[test]
    fn add_result_matches_wrapping(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.add(b);
        prop_assert_eq!(cpu.regs.a, a.wrapping_add(b));
    }

    #[test]
    fn add_zero_flag(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.add(b);
        prop_assert_eq!(cpu.get_z(), a.wrapping_add(b) == 0);
    }

    #[test]
    fn add_carry_flag(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.add(b);
        prop_assert_eq!(cpu.get_c(), (a as u16) + (b as u16) > 0xFF);
    }

    #[test]
    fn add_half_carry_flag(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.add(b);
        prop_assert_eq!(cpu.get_h(), (a & 0x0F) + (b & 0x0F) > 0x0F);
    }

    #[test]
    fn add_n_flag_always_clear(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.add(b);
        prop_assert!(!cpu.get_n());
    }

    #[test]
    fn sub_result_matches_wrapping(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.sub(b);
        prop_assert_eq!(cpu.regs.a, a.wrapping_sub(b));
    }

    #[test]
    fn sub_zero_flag(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.sub(b);
        prop_assert_eq!(cpu.get_z(), a == b);
    }

    #[test]
    fn sub_carry_flag(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.sub(b);
        prop_assert_eq!(cpu.get_c(), a < b);
    }

    #[test]
    fn sub_half_carry_flag(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.sub(b);
        prop_assert_eq!(cpu.get_h(), (a & 0x0F) < (b & 0x0F));
    }

    #[test]
    fn sub_n_flag_always_set(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.sub(b);
        prop_assert!(cpu.get_n());
    }

    #[test]
    fn adc_result(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.adc(b);
        let expected = a.wrapping_add(b).wrapping_add(carry as u8);
        prop_assert_eq!(cpu.regs.a, expected);
    }

    #[test]
    fn adc_carry_flag(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.adc(b);
        let sum = (a as u16) + (b as u16) + (carry as u16);
        prop_assert_eq!(cpu.get_c(), sum > 0xFF);
    }

    #[test]
    fn adc_half_carry_flag(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.adc(b);
        let sum = (a as u16) + (b as u16) + (carry as u16);
        prop_assert_eq!(cpu.get_h(), ((a as u16) ^ (b as u16) ^ sum) & 0x10 != 0);
    }

    #[test]
    fn sbc_result(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.sbc(b);
        let expected = a.wrapping_sub(b).wrapping_sub(carry as u8);
        prop_assert_eq!(cpu.regs.a, expected);
    }

    #[test]
    fn sbc_carry_flag(a: u8, b: u8, carry: bool) {
        let mut cpu = test_cpu(a, carry);
        cpu.sbc(b);
        prop_assert_eq!(cpu.get_c(), (a as u16) < (b as u16) + (carry as u16));
    }

    #[test]
    fn compare_does_not_modify_a(a: u8, b: u8) {
        let mut cpu = test_cpu(a, false);
        cpu.compare(b);
        prop_assert_eq!(cpu.regs.a, a);
    }

    #[test]
    fn compare_flags_match_sub(a: u8, b: u8) {
        let mut cpu1 = test_cpu(a, false);
        let mut cpu2 = test_cpu(a, false);
        cpu1.sub(b);
        cpu2.compare(b);
        prop_assert_eq!(cpu1.get_z(), cpu2.get_z());
        prop_assert_eq!(cpu1.get_n(), cpu2.get_n());
        prop_assert_eq!(cpu1.get_h(), cpu2.get_h());
        prop_assert_eq!(cpu1.get_c(), cpu2.get_c());
    }

    #[test]
    fn logical_or_result(a: u8, b: u8) {
        let mut cpu = test_cpu(a, true);
        cpu.logical_or(b);
        prop_assert_eq!(cpu.regs.a, a | b);
        prop_assert_eq!(cpu.get_z(), (a | b) == 0);
        prop_assert!(!cpu.get_n());
        prop_assert!(!cpu.get_h());
        prop_assert!(!cpu.get_c());
    }

    #[test]
    fn logical_and_result(a: u8, b: u8) {
        let mut cpu = test_cpu(a, true);
        cpu.logical_and(b);
        prop_assert_eq!(cpu.regs.a, a & b);
        prop_assert_eq!(cpu.get_z(), (a & b) == 0);
        prop_assert!(!cpu.get_n());
        prop_assert!(cpu.get_h());
        prop_assert!(!cpu.get_c());
    }

    #[test]
    fn logical_xor_result(a: u8, b: u8) {
        let mut cpu = test_cpu(a, true);
        cpu.logical_xor(b);
        prop_assert_eq!(cpu.regs.a, a ^ b);
        prop_assert_eq!(cpu.get_z(), (a ^ b) == 0);
        prop_assert!(!cpu.get_n());
        prop_assert!(!cpu.get_h());
        prop_assert!(!cpu.get_c());
    }

    #[test]
    fn inc_result(val: u8) {
        let mut cpu = Cpu::new();
        cpu.set_c(true);
        let result = cpu.inc(val);
        prop_assert_eq!(result, val.wrapping_add(1));
        prop_assert_eq!(cpu.get_z(), result == 0);
        prop_assert!(!cpu.get_n());
        prop_assert_eq!(cpu.get_h(), (val & 0x0F) + 1 > 0x0F);
        prop_assert!(cpu.get_c());
    }

    #[test]
    fn dec_result(val: u8) {
        let mut cpu = Cpu::new();
        cpu.set_c(true);
        let result = cpu.dec(val);
        prop_assert_eq!(result, val.wrapping_sub(1));
        prop_assert_eq!(cpu.get_z(), result == 0);
        prop_assert!(cpu.get_n());
        prop_assert_eq!(cpu.get_h(), (val & 0x0F) == 0);
        prop_assert!(cpu.get_c());
    }
}

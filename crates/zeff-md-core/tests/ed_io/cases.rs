use crate::multi_transfer_cases as multi;
use multi::{finish, fixture, start};
use zeff_emu_common::time::MachineTiming;

pub fn ed_io_grant_keeps_terminal_and_registers() {
    for opcode in [0x48, 0x60, 0x70, 0x71] {
        let mut machine = start(
            &[0x01, 0x34, 0x12, 0xed, opcode, 0x76],
            3,
            &[],
            &[(4, if opcode & 1 == 0 { 0x71 } else { 0x48 })],
        );
        assert_eq!(machine.sound_cpu().cycles(), 18);
        assert_eq!(machine.sound_cpu().regs().bc(), 0x1234);
        assert_eq!(machine.sound_cpu().regs().f, 0);
        assert!(machine.sound_instruction_pending());
        finish(&mut machine);
        assert_eq!(
            machine.sound_cpu().regs().c,
            if opcode == 0x48 { 0xff } else { 0x34 }
        );
        assert_eq!(
            machine.sound_cpu().regs().h,
            if opcode == 0x60 { 0xff } else { 0 }
        );
        assert_eq!(
            machine.sound_cpu().regs().f,
            if opcode & 1 == 0 { 0xac } else { 0 }
        );
        assert!(machine.sound_fault().is_none());
        assert_eq!(machine.sound_bank(), 0);
    }
}

pub fn ed_io_reset_cancels_pending_input() {
    let mut machine = start(&[0x01, 0x34, 0x12, 0xed, 0x48, 0x76], 3, &[], &[]);
    assert_eq!(machine.sound_cpu().cycles(), 18);
    let now = machine.timing_snapshot().now();
    let memory = machine.peek(0xa0_0004);
    machine.reset_sound_cpu();
    assert!(!machine.sound_instruction_pending());
    assert_eq!(machine.timing_snapshot().now(), now);
    assert_eq!(machine.peek(0xa0_0004), memory);
    assert_eq!(machine.sound_cpu().regs().c, 0);
    assert_eq!(machine.sound_cpu().regs().f, 0);
    assert!(machine.sound_fault().is_none());
}

pub fn ed_io_disconnected_ports_preserve_machine_state() {
    for port in [0u8, 0x34, 0xff] {
        for opcode in [
            0x40, 0x48, 0x50, 0x58, 0x60, 0x68, 0x70, 0x78, 0x41, 0x49, 0x51, 0x59, 0x61, 0x69,
            0x71, 0x79,
        ] {
            let mut machine = fixture::stopped(&[0x01, port, 0x12, 0xed, opcode, 0x76], &[]);
            machine.step_sound_instruction().unwrap();
            let main = machine.registers();
            let clock = machine.cpu_clock();
            let now = machine.timing_snapshot().now().get();
            let mut copied = machine.clone();
            let result = machine.step_sound_instruction();
            assert_eq!(result, copied.step_sound_instruction());
            assert_eq!(machine, copied);
            assert_eq!(result.unwrap().cycles, 12);
            assert_eq!(machine.registers(), main);
            assert_eq!(machine.cpu_clock(), clock);
            assert_eq!(machine.sound_bank(), 0);
            assert!(machine.sound_fault().is_none());
            assert_eq!(machine.peek(0xa0_0000), Some(0x01));
            assert_eq!(machine.timing_snapshot().now().get(), (now / 15 + 12) * 15);
            assert_eq!(
                machine.sound_cpu().regs().f,
                if opcode & 1 == 0 { 0xac } else { 0 }
            );
        }
    }
}

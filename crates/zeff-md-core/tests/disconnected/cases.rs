use crate::sound_fixture as fixture;
use fixture::*;
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_md_core::SoundFault;

pub fn disconnected_ports_consume_clocks() {
    for port in 0..=255u8 {
        let mut machine = stopped(&[0xd3, port, 0xdb, port, 0x76], &[]);
        let before = machine.timing_snapshot().now().get();
        let main_clock = machine.cpu_clock();
        let mut copied = machine.clone();
        for _ in 0..2 {
            let result = machine.step_sound_instruction();
            assert_eq!(result, copied.step_sound_instruction());
            assert_eq!(machine, copied);
            assert_eq!(result.unwrap().cycles, 11);
        }
        assert_eq!(machine.sound_cpu().regs().a, 0xff);
        assert_eq!(machine.sound_cpu().regs().pc, 4);
        assert_eq!(machine.sound_cpu().cycles(), 22);
        assert_eq!(machine.sound_bank(), 0);
        assert_eq!(machine.peek(SOUND_RAM), Some(0xd3));
        assert_eq!(machine.cpu_clock(), main_clock);
        assert_eq!(
            machine.timing_snapshot().now().get(),
            (before / 15 + 22) * 15
        );
    }
}

pub fn unused_sound_memory_and_main_lanes() {
    for address in [0x6100u16, 0x6101, 0x7000, 0x7efe, 0x7eff] {
        let [low, high] = address.to_le_bytes();
        let mut machine = stopped(&[0x3e, 0x55, 0x32, low, high, 0x3a, low, high, 0x76], &[]);
        for clocks in [7, 13, 13] {
            assert_eq!(machine.step_sound_instruction().unwrap().cycles, clocks);
        }
        assert_eq!(machine.sound_cpu().regs().a, 0xff);
        assert_eq!(machine.sound_bank(), 0);
        assert_eq!(machine.peek(SOUND_RAM + u32::from(address)), Some(0xff));
        let mut program = Vec::new();
        store_word(&mut program, BUS_REQUEST, 0x100);
        store_word(&mut program, RESET, 0x100);
        for target in [
            SOUND_RAM + u32::from(address),
            SOUND_RAM + u32::from(address) + 0x8000,
        ] {
            store_byte(&mut program, target, 0x55);
            program.extend_from_slice(&[0x1039, (target >> 16) as u16, target as u16]);
        }
        let aligned = (SOUND_RAM + u32::from(address)) & !1;
        store_word(&mut program, aligned, 0x1234);
        program.extend_from_slice(&[
            0x3239,
            (aligned >> 16) as u16,
            aligned as u16,
            0x4e71,
            0x4e71,
        ]);
        let mut machine = new(&program, &[]);
        for _ in 0..8 {
            instruction(&mut machine);
        }
        assert_eq!(machine.registers().d[0], 0xff);
        assert_eq!(machine.registers().d[1], 0xffff);
        assert_eq!(machine.sound_bank(), 0);
        assert_eq!(machine.peek(SOUND_RAM), Some(0));
    }
}

pub fn unsupported_devices_remain_faults() {
    for address in [0x4004u16, 0x5fff, 0x7f00, 0x7fff] {
        for write in [false, true] {
            let [low, high] = address.to_le_bytes();
            let mut machine = stopped(&[if write { 0x32 } else { 0x3a }, low, high], &[]);
            let saved = machine.clone();
            assert_eq!(
                machine.step_sound_instruction(),
                Err(SoundFault::UnsupportedAccess {
                    address,
                    direction: if write {
                        Direction::Write
                    } else {
                        Direction::Read
                    },
                })
            );
            assert_eq!(machine, saved);
        }
    }
}

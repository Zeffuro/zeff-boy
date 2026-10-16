use crate::sound_fixture as fixture;

use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::TimingFault;
use zeff_md_core::{BusError, ControllerKind, ControllerPort, Emulator, LoadConfig, MachineFault};

const ABSENT_VIEWS: [u32; 8] = [
    0x40_0000, 0x7f_fffe, 0x80_0000, 0x9f_fffe, 0xa1_2000, 0xa1_5100, 0xb0_0000, 0xbf_fffe,
];
const PRIOR_WRITE: u32 = 0xff_8004;

pub fn absent_views_reject_main_transfers_and_preserve_completed_effects() {
    let mut rows = 0;
    for concurrent_sound in [false, true] {
        for high in [0, 0x1200_0000] {
            for base in ABSENT_VIEWS {
                for (opcode, direction, byte) in [
                    (0x1039, Direction::Read, true),
                    (0x3039, Direction::Read, false),
                    (0x2039, Direction::Read, false),
                    (0x13c0, Direction::Write, true),
                    (0x33c0, Direction::Write, false),
                    (0x23c0, Direction::Write, false),
                ] {
                    for lower in 0..=u32::from(byte) {
                        let address = high | (base + lower);
                        let mut program = Vec::new();
                        fixture::store_byte(&mut program, PRIOR_WRITE, 0x5a);
                        program.extend_from_slice(&[
                            0x7055,
                            opcode,
                            (address >> 16) as u16,
                            address as u16,
                            0x4e71,
                            0x4e71,
                        ]);
                        let mut machine = Emulator::new(
                            fixture::image(&program, &[]),
                            LoadConfig {
                                concurrent_sound,
                                ..LoadConfig::default()
                            },
                        )
                        .unwrap();
                        fixture::instruction(&mut machine);
                        fixture::instruction(&mut machine);
                        machine
                            .input_mut()
                            .set_controller_kind(ControllerPort::One, ControllerKind::SixButton);
                        machine
                            .input_mut()
                            .set_controller_extra_raw(ControllerPort::One, 5);
                        let before = machine.timing_snapshot().now().get();
                        let clock = machine.cpu_clock().0;
                        let mut copied = machine.clone();
                        let result = machine.step_instruction();
                        assert_eq!(result, copied.step_instruction());
                        assert_eq!(machine, copied);
                        assert!(matches!(
                            result,
                            Err(MachineFault::Main(TimingFault::Bus {
                                source: BusError::UnsupportedAccess { address, direction: actual },
                                ..
                            })) if address == base && actual == direction
                        ));
                        assert!(machine.cpu_clock().0 > clock);
                        assert_eq!(
                            machine.timing_snapshot().now().get() - before,
                            (machine.cpu_clock().0 - clock)
                                * zeff_md_core::constants::MAIN_CPU_DIVISOR
                        );
                        assert_eq!(machine.registers().d[0], 0x55);
                        assert_eq!(machine.peek(PRIOR_WRITE), Some(0x5a));
                        assert_eq!(machine.peek(PRIOR_WRITE - 1), Some(0));
                        assert_eq!(machine.peek(PRIOR_WRITE + 1), Some(0));
                        assert_eq!(machine.input().controller_extra_raw(ControllerPort::One), 5);
                        assert!(machine.sound_reset_asserted());
                        let saved = machine.clone();
                        assert_eq!(
                            machine.step_instruction(),
                            Err(MachineFault::Main(TimingFault::Frozen))
                        );
                        assert_eq!(machine, saved);
                        rows += 1;
                    }
                }
            }
        }
    }
    assert_eq!(rows, 256);
}

pub fn absent_views_do_not_supply_peek_backing() {
    let machine = fixture::new(&[0x4e71, 0x4e71], &[]);
    let saved = machine.clone();
    for high in [0, 0x1200_0000] {
        for address in ABSENT_VIEWS {
            for lower in [0, 1] {
                assert_eq!(machine.peek(high | (address + lower)), None);
            }
        }
    }
    assert_eq!(machine, saved);
}

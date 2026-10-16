use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::Direction;
use zeff_m68k::timing::TimingFault;
use zeff_md_core::{
    BusError, ControlCommand, ControlWrite, Emulator, LoadConfig, MachineFault, VdpControl,
};

pub const ADDRESSES: [u16; 6] = [0, 1, 0x3fff, 0x4000, 0x8001, 0xffff];
pub const GUARDED_PORTS: [u32; 7] = [
    0xc00000, 0xc00002, 0xc00004, 0xc00006, 0xc00008, 0xd8ff04, 0xc00020,
];

pub fn command(address: u16, code: u8) -> ControlWrite {
    ControlWrite::Command(ControlCommand {
        address: u32::from(address),
        code,
    })
}

pub fn words(address: u16, code: u8) -> [u16; 2] {
    [
        (address & 0x3fff) | (u16::from(code & 3) << 14),
        (address >> 14) | (u16::from(code >> 2) << 4),
    ]
}

pub fn literal_command_fields() {
    for (first, second, address, code) in [
        (0x0000, 0x0000, 0x0000, 0x00),
        (0x7fff, 0x0003, 0xffff, 0x01),
        (0xc0fe, 0x0000, 0x00fe, 0x03),
        (0x4000, 0x0010, 0x0000, 0x05),
        (0x0123, 0x0022, 0x8123, 0x08),
        (0x0123, 0x0011, 0x4123, 0x04),
        (0x4567, 0x0003, 0xc567, 0x01),
        (0x4567, 0x8f02, 0x8567, 0x01),
        (0xc07f, 0x0020, 0x007f, 0x0b),
        (0x4000, 0x0080, 0x0000, 0x21),
        (0xffff, 0xffff, 0x1ffff, 0x3f),
    ] {
        let mut decoder = VdpControl::default();
        assert!(matches!(
            decoder.write_control(first),
            ControlWrite::FirstHalf { .. }
        ));
        assert_eq!(decoder.pending_word(), Some(first));
        assert_eq!(
            decoder.write_control(second),
            ControlWrite::Command(ControlCommand { address, code })
        );
        assert_eq!(decoder.pending_word(), None);
    }
}

pub fn register_requests_and_ignored_tag_bit() {
    for index in 0u8..32 {
        for value in 0u8..=255 {
            for tag in [0x8000, 0xa000] {
                let mut decoder = VdpControl::default();
                assert_eq!(
                    decoder.write_control(tag | (u16::from(index) << 8) | u16::from(value)),
                    ControlWrite::Register { index, value }
                );
                assert_eq!(decoder.pending_word(), None);
            }
        }
    }
}

pub fn command_and_register_encoding_partition() {
    for code in 0..64 {
        for address in ADDRESSES {
            let [first, second] = words(address, code);
            let mut decoder = VdpControl::default();
            if code & 3 == 2 {
                assert_eq!(
                    decoder.write_control(first),
                    ControlWrite::Register {
                        index: ((address >> 8) & 31) as u8,
                        value: address as u8,
                    }
                );
                assert_eq!(decoder.pending_word(), None);
            } else {
                assert_eq!(
                    decoder.write_control(first),
                    ControlWrite::FirstHalf {
                        address_low: address & 0x3fff,
                        code_low: code & 3,
                    }
                );
                assert_eq!(decoder.write_control(second), command(address, code));
                assert_eq!(decoder.pending_word(), None);
            }
        }
    }
}

pub fn register_pattern_after_first_half_is_command_data() {
    for index in 0u16..32 {
        for tag in [0x8000, 0xa000] {
            let mut decoder = VdpControl::default();
            decoder.write_control(0x4567);
            assert_eq!(
                decoder.write_control(tag | (index << 8) | 2),
                command(0x8567, 1)
            );
            assert_eq!(
                decoder.write_control(0x8f02),
                ControlWrite::Register {
                    index: 15,
                    value: 2
                }
            );
        }
    }
}

pub fn unused_second_word_bits_do_not_change_request() {
    for unused in [
        0, 8, 0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000, 0x4000, 0x8000, 0xff08,
    ] {
        let mut decoder = VdpControl::default();
        decoder.write_control(0x4567);
        assert_eq!(
            decoder.write_control(0x00b2 | unused),
            command(0x8567, 0x2d)
        );
    }
}

pub fn cancellation_and_pending_copies() {
    let mut decoder = VdpControl::default();
    decoder.cancel_pending();
    assert_eq!(decoder, VdpControl::default());
    decoder.write_control(0x4567);
    let mut copied = decoder;
    decoder.cancel_pending();
    decoder.cancel_pending();
    assert_eq!(decoder.pending_word(), None);
    assert_eq!(
        decoder.write_control(0x8f02),
        ControlWrite::Register {
            index: 15,
            value: 2
        }
    );
    assert_eq!(copied.pending_word(), Some(0x4567));
    assert_eq!(copied.write_control(0x8f02), command(0x8567, 1));
    assert_eq!(decoder, copied);
    decoder.write_control(0xc07f);
    let mut copied = decoder;
    assert_eq!(decoder.write_control(0x0020), copied.write_control(0x0020));
    assert_eq!(decoder, copied);
}

pub fn independent_decoder_instances() {
    let mut main = VdpControl::default();
    let mut other = VdpControl::default();
    main.write_control(0x4567);
    assert_eq!(
        other.write_control(0x8f02),
        ControlWrite::Register {
            index: 15,
            value: 2
        }
    );
    other.write_control(0xc07f);
    assert_eq!(main.write_control(0x0003), command(0xc567, 1));
    assert_eq!(other.write_control(0x0020), command(0x007f, 0x0b));
    assert_eq!(main, other);
}

pub fn machine_start(address: u32, direction: Direction, concurrent_sound: bool) -> Emulator {
    let mut program = vec![0x33fc, 0xa55a, 0x00ff, 0x8000];
    match direction {
        Direction::Write => program.extend([0x33fc, 0x8f02]),
        Direction::Read => program.push(0x3039),
    }
    program.extend([(address >> 16) as u16, address as u16, 0x4e71, 0x4e71]);
    let mut rom = vec![0; 0x200];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.into_iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        rom,
        LoadConfig {
            concurrent_sound,
            ..Default::default()
        },
    )
    .unwrap()
}

pub fn machine_ports_remain_unadmitted() {
    for concurrent in [false, true] {
        for address in GUARDED_PORTS {
            for direction in [Direction::Read, Direction::Write] {
                let mut machine = machine_start(address, direction, concurrent);
                assert!(machine.step_instruction().is_ok());
                let before = machine.timing_snapshot().now();
                let result = machine.step_instruction();
                if direction == Direction::Write
                    && [0xc00004, 0xc00006, 0xd8ff04].contains(&address)
                {
                    assert!(result.is_ok());
                    assert_eq!(machine.vdp().registers().registers()[15], Some(2));
                    assert_eq!(machine.vdp().control().pending_word(), None);
                    assert_eq!(
                        machine.vdp().command(),
                        ControlCommand {
                            address: 0x0f02,
                            code: 0x12
                        }
                    );
                    assert_eq!(machine.peek(address), None);
                    continue;
                }
                if direction == Direction::Read && [0xc00004, 0xc00006, 0xd8ff04].contains(&address)
                {
                    assert!(result.is_ok(), "{result:?}");
                    assert_eq!(machine.vdp().control().pending_word(), None);
                    continue;
                }
                if direction == Direction::Read && address == 0xc00008 {
                    assert!(result.is_ok(), "{result:?}");
                    assert_eq!(machine.peek(address), None);
                    continue;
                }
                if [0xc00000, 0xc00002].contains(&address) {
                    assert!(matches!(
                        result,
                        Err(MachineFault::Main(TimingFault::Bus {
                            source: BusError::VdpPort(_),
                            ..
                        }))
                    ));
                } else {
                    assert!(matches!(result, Err(MachineFault::Main(TimingFault::Bus {
                    source: BusError::UnsupportedAccess { address: actual, direction: actual_direction }, ..
                })) if actual == address && actual_direction == direction));
                }
                assert!(machine.timing_snapshot().now() > before);
                assert_eq!(machine.peek(address), None);
                assert_eq!(machine.peek(0xff8000), Some(0xa5));
                assert_eq!(machine.peek(0xff8001), Some(0x5a));
                let copied = machine.clone();
                assert_eq!(
                    machine.step_instruction(),
                    Err(MachineFault::Main(TimingFault::Frozen))
                );
                assert_eq!(machine, copied);
            }
        }
    }
}

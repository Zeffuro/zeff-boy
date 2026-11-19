use super::*;
use crate::cartridge::linear_rom::{LinearRomOnly, fixture};
use crate::hardware::sound::{CandidateBus, SoundMemory, schedule::PendingSound};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;
use zeff_z80::Z80Bus;

const LEN: u32 = 0x100000;
const STORE: [u16; 4] = [0x13fc, 0x5a, 0x00ff, 0x0080];

fn machine(words: &[u16], concurrent_sound: bool) -> Emulator {
    let cart = fixture(words);
    let mut result = Emulator::new(
        cart.bytes.to_vec(),
        LoadConfig {
            concurrent_sound,
            ..LoadConfig::default()
        },
    )
    .unwrap();
    result.bus.cartridge = cart;
    result
}

fn identity() {
    additional_identities();
    let expected = [
        0xe0, 0x5e, 0x03, 0xf0, 0xb3, 0x37, 0x75, 0xe5, 0xcc, 0x84, 0x4c, 0x5a, 0xdb, 0x8f, 0xed,
        0xd4, 0x1a, 0xbb, 0x72, 0x1f, 0xd3, 0x77, 0x8f, 0xf0, 0x72, 0x09, 0x21, 0x12, 0xba, 0x5c,
        0xbc, 0xb9,
    ];
    let declaration = *b"RA          ";
    assert!(LinearRomOnly::identity(
        LEN as usize,
        Some(&declaration),
        || expected
    ));
    for index in 0..32 {
        for bit in 0..8 {
            let mut other = expected;
            other[index] ^= 1 << bit;
            assert!(!LinearRomOnly::identity(
                LEN as usize,
                Some(&declaration),
                || other
            ));
        }
    }
    for len in [0, 0x200, 0xfffff, 0x100001, 0x200000] {
        assert!(!LinearRomOnly::identity(
            len,
            Some(&declaration),
            || panic!("wrong size")
        ));
    }
    for index in 0..12 {
        let mut other = declaration;
        other[index] ^= 1;
        assert!(!LinearRomOnly::identity(
            LEN as usize,
            Some(&other),
            || panic!("wrong descriptor")
        ));
    }
    assert!(!LinearRomOnly::identity(LEN as usize, None, || panic!(
        "missing descriptor"
    )));
    let cart = fixture(&[0x4e71, 0x60fc]);
    let mut bytes = cart.bytes.to_vec();
    bytes[0x1b0..0x1bc].copy_from_slice(&declaration);
    assert!(LinearRomOnly::detect(&bytes).is_none());
    assert_eq!(
        Cartridge::new(bytes).unwrap_err(),
        LoadError::UnsupportedSaveHardware
    );
    let ordinary = Cartridge::new(cart.bytes.to_vec()).unwrap();
    assert!(ordinary.linear_rom.is_none());
    assert_eq!(ordinary.read(LEN), Some(0xff));
    assert_eq!(cart.read(LEN), None);
    assert!(cart.save.is_none());
    assert_eq!(cart, cart.clone());
    assert!(format!("{cart:?}").contains("LinearRomOnly"));
}

fn additional_identities() {
    for (size, declaration, expected) in [
        (
            1048576,
            [
                0x52, 0x41, 0xe0, 0x20, 0x00, 0x20, 0x00, 0x01, 0x00, 0x20, 0x3f, 0xff,
            ],
            [
                0x28, 0x63, 0x52, 0x07, 0x56, 0xf6, 0x9e, 0x87, 0x28, 0x52, 0x85, 0x59, 0xc8, 0x2f,
                0xed, 0x99, 0x11, 0x87, 0xed, 0x1f, 0x60, 0x83, 0xe7, 0x8a, 0xa4, 0xfe, 0x27, 0x2d,
                0xf1, 0x1a, 0x34, 0x02,
            ],
        ),
        (
            524288,
            [
                0x52, 0x41, 0xf8, 0x20, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x3f, 0xff,
            ],
            [
                0x76, 0x1b, 0xbe, 0xa2, 0x8a, 0x5e, 0x4b, 0xbe, 0x12, 0xd0, 0x18, 0xb3, 0x44, 0x68,
                0x02, 0xca, 0xbf, 0xec, 0xbd, 0x3a, 0x0a, 0x0c, 0x48, 0xc3, 0xe3, 0x65, 0x09, 0x57,
                0x8c, 0x91, 0xdb, 0x6b,
            ],
        ),
    ] {
        let owner =
            LinearRomOnly::matching_identity(size, Some(&declaration), || expected).unwrap();
        assert!(owner.matches_digest(expected));
        for index in 0..32 {
            for bit in 0..8 {
                let mut other = expected;
                other[index] ^= 1 << bit;
                assert!(!owner.matches_digest(other));
                assert!(!LinearRomOnly::identity(size, Some(&declaration), || other));
            }
        }
        for length in [0, 0x200, size - 1, size + 1, 0x400000] {
            assert!(!LinearRomOnly::identity(
                length,
                Some(&declaration),
                || panic!("wrong size")
            ));
        }
        for index in 0..12 {
            let mut other = declaration;
            other[index] ^= 1;
            assert!(!LinearRomOnly::identity(size, Some(&other), || panic!(
                "wrong descriptor"
            )));
        }
        let cart = crate::cartridge::linear_rom::fixture_with_len(&[0x4e71, 0x60fc], size);
        assert_eq!(cart.read((size - 1) as u32), Some(0x78));
        for at in [
            size as u32,
            (size as u32).max(0x80000),
            (size as u32).max(0x9ffff),
            0x200000,
        ] {
            assert_eq!(cart.read(at), None);
        }
        let mut bytes = cart.bytes.to_vec();
        bytes[0x1b0..0x1bc].copy_from_slice(&declaration);
        assert!(LinearRomOnly::detect(&bytes).is_none());
        assert_eq!(
            Cartridge::new(bytes).unwrap_err(),
            LoadError::UnsupportedSaveHardware
        );
    }
}

fn main_transfers() {
    for concurrent in [false, true] {
        for high in [0, 0x12000000] {
            for (opcode, at, expected, mask) in [
                (0x1039, LEN - 1, 0x78, 0xff),
                (0x3039, LEN - 2, 0x5678, 0xffff),
                (0x2039, LEN - 4, 0x12345678, u32::MAX),
            ] {
                let target = high | at;
                let words: Vec<_> = [opcode, (target >> 16) as u16, target as u16]
                    .into_iter()
                    .chain(STORE)
                    .chain([0x60fe])
                    .collect();
                let mut source = machine(&words, concurrent);
                let mut copied = source.clone();
                assert_eq!(source.step_instruction(), copied.step_instruction());
                assert_eq!(source, copied);
                assert_eq!(source.registers().d[0] & mask, expected);
                source.step_instruction().unwrap();
                assert_eq!(source.peek(0xff0080), Some(0x5a));
            }
            for (opcode, base, direction) in [
                (0x1039, LEN, Direction::Read),
                (0x1039, LEN + 1, Direction::Read),
                (0x3039, LEN, Direction::Read),
                (0x2039, LEN - 2, Direction::Read),
                (0x3039, 0x200000, Direction::Read),
                (0x13c0, 0, Direction::Write),
                (0x13c0, LEN - 1, Direction::Write),
                (0x33c0, 0x200, Direction::Write),
                (0x23c0, LEN - 2, Direction::Write),
                (0x33c0, LEN, Direction::Write),
                (0x33c0, 0x200000, Direction::Write),
                (0x33c0, 0x3ffffe, Direction::Write),
                (0x13c0, 0x400001, Direction::Write),
                (0x23c0, 0x9ffffc, Direction::Write),
                (0x13c0, 0xa130f1, Direction::Write),
                (0x33c0, 0xa130f0, Direction::Write),
            ] {
                let target = high | base;
                let words: Vec<_> = [opcode, (target >> 16) as u16, target as u16]
                    .into_iter()
                    .chain(STORE)
                    .chain([0x60fe])
                    .collect();
                let mut source = machine(&words, concurrent);
                source.cpu.registers.d[0] = 0x55;
                let bytes = source.bus.cartridge.bytes.clone();
                assert!(
                    matches!(source.step_instruction(), Err(MachineFault::Main(TimingFault::Bus {
                    source: BusError::UnsupportedAccess { direction: actual, .. }, ..
                })) if actual == direction)
                );
                assert_eq!(source.peek(0xff0080), Some(0));
                assert_eq!(source.bus.cartridge.bytes, bytes);
                let faulted = source.clone();
                assert_eq!(
                    source.step_instruction(),
                    Err(MachineFault::Main(TimingFault::Frozen))
                );
                assert_eq!(source, faulted);
            }
        }
    }
    for high in [0, 0x12000000] {
        let mut source = machine(&[0x4e71, 0x60fc], false);
        for address in [LEN - 1, LEN, 0x200001, 0x400001, 0x9fffff] {
            assert_eq!(
                source.peek(high | address),
                if address < LEN { Some(0x78) } else { None }
            );
        }
        for kind in [AccessKind::Data, AccessKind::Prefetch] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                for address in [LEN - 2, LEN, 0x200000] {
                    let access = BusAccess {
                        logical_address: high | address,
                        bus_address: address,
                        kind,
                        lanes,
                        function_code: FunctionCode::SupervisorProgram,
                    };
                    let start = source.bus.cpu_clock;
                    assert_eq!(
                        source.bus.read_cycle(access, start).result.is_ok(),
                        address < LEN
                    );
                }
            }
        }
    }
}

fn sound_program(target: u32, write: bool) -> (u16, Vec<u8>) {
    let address = 0x8000 | (target as u16 & 0x7fff);
    let [low, high] = address.to_le_bytes();
    (
        address,
        vec![
            if write { 0x32 } else { 0x3a },
            low,
            high,
            0x3e,
            0x5a,
            0x32,
            0x77,
            0,
            0x76,
        ],
    )
}

fn providers() {
    let cart = fixture(&[0x4e71, 0x60fc]);
    for target in [0x200, LEN - 1, LEN, 0x200000, 0x3fffff, 0x400000, 0x9fffff] {
        for write in [false, true] {
            let (address, program) = sound_program(target, write);
            let good = !write && target < LEN;
            let mut memory = SoundMemory {
                bank: (target >> 15) as u16,
                ..SoundMemory::default()
            };
            memory.set_reset(false);
            memory.ram[..program.len()].copy_from_slice(&program);
            let before = memory.clone();
            {
                let mut bus = CandidateBus::new(&mut memory, &cart);
                if write {
                    bus.cpu_write(address, 0x42);
                } else {
                    let value = bus.cpu_read(address);
                    if good {
                        assert_eq!(Some(value), cart.read(target));
                    }
                }
                assert_eq!(
                    bus.fault.get(),
                    if good {
                        None
                    } else {
                        Some(SoundFault::UnsupportedAccess {
                            address,
                            direction: if write {
                                Direction::Write
                            } else {
                                Direction::Read
                            },
                        })
                    }
                );
                assert_eq!(bus.bank_access.get().is_some(), good);
                assert_eq!(bus.cartridge_read.get(), None);
                assert_eq!(&*bus.memory, &before);
            }
            let cpu = zeff_z80::Cpu::new();
            let prepared = PendingSound::prepare(&cpu, &memory, &cart, 0);
            assert_eq!(prepared.is_ok(), good);
            assert_eq!(cpu, zeff_z80::Cpu::new());
            assert_eq!(memory, before);
            for concurrent in [false, true] {
                let words = if concurrent {
                    &[0x4e71, 0x60fc][..]
                } else {
                    &[0x4e72, 0x2700, 0x60fe][..]
                };
                let mut machine = machine(words, concurrent);
                if !concurrent {
                    machine.step_instruction().unwrap();
                    assert_eq!(
                        machine.step_instruction().unwrap().cpu,
                        TimedOutcome::Stopped
                    );
                }
                machine.bus.sound = before.clone();
                if concurrent {
                    for _ in 0..200 {
                        if machine.sound_fault().is_some() || machine.bus.sound.ram[0x77] == 0x5a {
                            break;
                        }
                        if let Err(error) = machine.step_instruction() {
                            assert!(!good);
                            assert!(matches!(
                                error,
                                MachineFault::Sound(SoundFault::UnsupportedAccess { .. })
                            ));
                            break;
                        }
                    }
                    assert_eq!(machine.sound_fault().is_none(), good);
                    if !good {
                        assert!(machine.sound_schedule.pending.is_none());
                    }
                } else {
                    let saved_cpu = machine.sound_cpu.clone();
                    let result = machine.step_sound_instruction();
                    assert_eq!(result.is_ok(), good);
                    if !good {
                        assert_eq!(machine.sound_cpu, saved_cpu);
                    }
                }
                if !good {
                    assert_eq!(machine.bus.sound.ram[0x77], 0);
                }
            }
        }
    }
}

fn restores() {
    let words = [0x4e71, 0x60fc];
    for concurrent in [false, true] {
        let source = machine(&words, concurrent);
        let raw = source.encode_state().unwrap();
        let front = source.encode_frontend_state().unwrap();
        assert_eq!(
            u32::from_le_bytes(raw[8..12].try_into().unwrap()),
            crate::save_state::STATE_VERSION
        );
        let mut raw_control = source.clone();
        raw_control.decode_state(&raw).unwrap();
        let mut raw_restored = machine(&words, concurrent);
        raw_restored.decode_state(&raw).unwrap();
        let mut front_control = source.clone();
        let mut front_restored = machine(&words, concurrent);
        front_restored.decode_frontend_state(&front).unwrap();
        for (a, b) in [
            (&mut raw_control, &mut raw_restored),
            (&mut front_control, &mut front_restored),
        ] {
            for _ in 0..8 {
                assert_eq!(a.step_instruction(), b.step_instruction());
                assert_eq!(a.encode_state().unwrap(), b.encode_state().unwrap());
                assert_eq!(
                    a.encode_frontend_state().unwrap(),
                    b.encode_frontend_state().unwrap()
                );
                let bits = |m: &mut Emulator| {
                    m.drain_audio_samples()
                        .into_iter()
                        .map(f32::to_bits)
                        .collect::<Vec<_>>()
                };
                assert_eq!(bits(a), bits(b));
                assert_eq!(a.framebuffer(), b.framebuffer());
            }
            Reset::reset(a);
            Reset::reset(b);
            assert_eq!(a, b);
            assert!(a.bus.cartridge.linear_rom.is_some());
            assert!(a.bus.cartridge.save.is_none());
            assert_eq!(a.peek(LEN), None);
            assert!(a.check_warm_reset().is_err());
        }
        let mut wrong = machine(&words, concurrent);
        std::sync::Arc::make_mut(&mut wrong.bus.cartridge.bytes)[0xfffff] ^= 1;
        let before = wrong.clone();
        assert!(wrong.decode_state(&raw).is_err());
        assert!(wrong.decode_frontend_state(&front).is_err());
        assert!(wrong.encode_state().is_err());
        assert_eq!(wrong, before);
        let mut target = source.clone();
        let before = target.clone();
        assert!(target.decode_state(&raw[..raw.len() - 1]).is_err());
        assert!(
            target
                .decode_frontend_state(&front[..front.len() - 1])
                .is_err()
        );
        assert_eq!(target, before);
    }
    for write in [false, true] {
        let mut ordinary = machine(&words, true);
        ordinary.bus.cartridge.linear_rom = None;
        let (_, program) = sound_program(LEN, write);
        ordinary.bus.sound.set_reset(false);
        ordinary.bus.sound.bank = (LEN >> 15) as u16;
        ordinary.bus.sound.ram[..program.len()].copy_from_slice(&program);
        ordinary.sound_schedule.pending = Some(
            PendingSound::prepare(
                &ordinary.sound_cpu,
                &ordinary.bus.sound,
                &ordinary.bus.cartridge,
                ordinary.bus.now.get(),
            )
            .unwrap(),
        );
        let encoded = ordinary.encode_state().unwrap();
        let frontend = ordinary.encode_frontend_state().unwrap();
        let mut strict = machine(&words, true);
        let before = strict.clone();
        assert!(strict.decode_state(&encoded).is_err());
        assert!(strict.decode_frontend_state(&frontend).is_err());
        assert_eq!(strict, before);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn partial_linear_rom_rejects_unqualified_accesses_and_preserves_codec_boundaries() {
    identity();
    main_transfers();
    providers();
    restores();
}

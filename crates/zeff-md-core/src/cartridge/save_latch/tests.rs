use super::*;

pub(crate) fn image(program: &[u16]) -> Vec<u8> {
    let mut rom = vec![0; ROM_BYTES];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x80..0x90].copy_from_slice(b"LATCHED_SAVE_TST");
    rom[0x100..0x104].copy_from_slice(b"SEGA");
    rom[0x18e..0x190].copy_from_slice(&0xba1fu16.to_be_bytes());
    rom[0x1b0..0x1bc].copy_from_slice(DECLARATION);
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + 2 * index..0x202 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    rom[0x1234] = 0x67;
    rom[ROM_BYTES - 1] = 0x89;
    rom
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn control(cart: &mut Cartridge, lanes: ByteLanes, value: u8) {
    let address = ram::SAVE_CONTROL + u32::from(lanes == ByteLanes::Lower);
    let data = if lanes == ByteLanes::Upper {
        u16::from(value) << 8 | 0x5a
    } else {
        0xa500 | u16::from(value)
    };
    assert_eq!(
        cart.write_save_latch(access(address, lanes), data, 0, 1),
        Some(true)
    );
}

fn identity() {
    let mut rom = image(&[]);
    rom[0x80..0x90].fill(0);
    let mut header = CartridgeHeader::parse(&rom).unwrap();
    header.calculated_checksum = 0xba1f;
    assert!(SaveLatch::detect(&rom, Some(&header), || IDENTITY).is_some());
    for index in 0..32 {
        let mut digest = IDENTITY;
        digest[index] ^= 1;
        assert!(SaveLatch::detect(&rom, Some(&header), || digest).is_none());
    }
    for index in 0x1b0..0x1bc {
        let mut bad = rom.clone();
        bad[index] ^= 1;
        assert!(
            SaveLatch::detect(&bad, Some(&header), || panic!("unmatched save hashed")).is_none()
        );
    }
    for (stored, calculated) in [(0xba1e, 0xba1f), (0xba1f, 0xba1e)] {
        let mut bad = header.clone();
        bad.checksum = stored;
        bad.calculated_checksum = calculated;
        assert!(
            SaveLatch::detect(&rom, Some(&bad), || panic!("unmatched checksum hashed")).is_none()
        );
    }
    for size in [0x100000, 0x1fffff, 0x200001, 0x300000] {
        let mut bad = rom.clone();
        bad.resize(size, 0);
        assert!(
            SaveLatch::detect(&bad, Some(&header), || panic!("unmatched size hashed")).is_none()
        );
    }
    assert!(Cartridge::new(rom).unwrap().save_latch.is_none());
}

fn mapping(template: &Cartridge) {
    for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
        for value in 0..=u8::MAX {
            let mut cart = template.clone();
            cart.save.as_mut().unwrap().bytes[..2].copy_from_slice(&[0x5a, 0x6b]);
            control(&mut cart, lanes, value);
            let enabled = value & 1 != 0;
            let protected = enabled && value & 2 != 0;
            let save = cart.save.as_ref().unwrap();
            assert_eq!((save.enabled, save.write_protected), (enabled, protected));
            assert!(!save.banked_readable());
            assert_eq!(cart.read(0x200001), Some(if enabled { 0x5a } else { 0xff }));
            assert_eq!(cart.rom_source_offset(0x200001), (!enabled).then_some(1));
            for address in [
                0, 1, 0x1234, 0x1fffff, 0x201234, 0x20ffff, 0x210000, 0x3fffff,
            ] {
                if enabled && SaveLatch::save_address(address) {
                    continue;
                }
                let offset = address as usize & (ROM_BYTES - 1);
                assert_eq!(cart.rom_source_offset(address), Some(offset));
                assert_eq!(cart.read(address), Some(cart.bytes[offset]));
            }
            assert_eq!(cart.rom_source_offset(0x400000), None);
            assert_eq!(cart.read_save_latch(0x400000), None);
            for (address, selected, data) in [
                (0x200000, ByteLanes::Upper, 0x1100),
                (0x200001, ByteLanes::Lower, 0x22),
                (0x200002, ByteLanes::Both, 0x3344),
                (0x20ffff, ByteLanes::Lower, 0x55),
                (0x210001, ByteLanes::Lower, 0x66),
            ] {
                assert_eq!(
                    cart.write_save_latch(access(address, selected), data, 0, 1),
                    Some(true)
                );
            }
            let bytes = &cart.save.as_ref().unwrap().bytes;
            assert_eq!(bytes[0], if enabled && !protected { 0x22 } else { 0x5a });
            assert_eq!(bytes[1], if enabled && !protected { 0x44 } else { 0x6b });
            assert_eq!(
                bytes[32767],
                if enabled && !protected { 0x55 } else { 0xff }
            );
            assert_eq!(cart.bytes, template.bytes);
        }
    }
}

fn guards(template: &Cartridge) {
    let request = access(ram::SAVE_CONTROL, ByteLanes::Upper);
    let mut requests = Vec::new();
    for address in [0xa13000, 0xa130ee, 0xa130f2, 0xa130fe] {
        requests.push(access(address, ByteLanes::Both));
    }
    for function_code in [
        FunctionCode::UserProgram,
        FunctionCode::SupervisorProgram,
        FunctionCode::CpuSpace,
    ] {
        requests.push(BusAccess {
            function_code,
            ..request
        });
    }
    for kind in [
        AccessKind::Prefetch,
        AccessKind::Discard,
        AccessKind::Stack,
        AccessKind::InterruptAcknowledge,
    ] {
        requests.push(BusAccess { kind, ..request });
    }
    requests.extend([
        BusAccess {
            logical_address: ram::SAVE_CONTROL + 1,
            ..request
        },
        BusAccess {
            logical_address: ram::SAVE_CONTROL | 0x1000000,
            ..request
        },
        BusAccess {
            bus_address: ram::SAVE_CONTROL + 1,
            ..request
        },
        access(ram::SAVE_CONTROL, ByteLanes::Lower),
    ]);
    for request in requests {
        let mut cart = template.clone();
        let before = cart.clone();
        assert_eq!(cart.write_save_latch(request, 0, 0, 1), Some(false));
        assert_eq!(cart, before);
    }
    let mut stripped = template.clone();
    stripped.save_latch = None;
    assert_ne!(stripped, *template);
    assert!(stripped.validate_save_latch().is_err());
    let mut changed = template.clone();
    *Arc::make_mut(&mut changed.bytes).last_mut().unwrap() ^= 1;
    assert!(changed.validate_save_latch().is_err());
    let mut conflict = template.clone();
    conflict.parallel_adapter = Some(parallel_adapter::ParallelAdapter::new(
        parallel_adapter::Model::First,
    ));
    assert!(conflict.validate_save_latch().is_err());
    let mut missing = template.clone();
    missing.save = None;
    assert!(missing.validate_save_latch().is_err());
    let mut wrong_save = template.clone();
    wrong_save.save.as_mut().unwrap().bytes = vec![0xff; 8192].into();
    assert!(wrong_save.validate_save_latch().is_err());
    let mut disabled = template.clone();
    control(&mut disabled, ByteLanes::Both, 0);
    assert!(disabled.save.as_ref().unwrap().validate_state().is_err());
    disabled.validate_save_latch().unwrap();
    let mut wrong = disabled.clone();
    wrong.save.as_mut().unwrap().write_protected = true;
    assert!(wrong.validate_save_latch().is_err());
}

fn sound_mapping(template: &Cartridge) {
    use crate::hardware::sound::{CandidateBus, SoundFault, SoundMemory};
    use zeff_emu_common::cheats::{CheatPatch, CheatValue};
    use zeff_z80::Z80Bus;

    for (value, active) in [0, 1, 3]
        .into_iter()
        .flat_map(|value| [false, true].map(|active| (value, active)))
    {
        let mut cart = template.clone();
        control(&mut cart, ByteLanes::Both, value);
        if active {
            cart.rom_overlay = cart
                .prepare_rom_overlay(&[(0x1234, 0xa5), (0x11234, 0xb6), (0x1fffff, 0xc7)].map(
                    |(address, value)| CheatPatch::WideRomWrite {
                        address,
                        value: CheatValue::Constant(value),
                    },
                ))
                .unwrap();
        }
        for target in [0x1234, 0x200003, 0x201234, 0x211234, 0x3fffff] {
            let mut memory = SoundMemory {
                bank: (target >> 15) as u16,
                ..SoundMemory::default()
            };
            let address = (0x8000 | (target & 0x7fff)) as u16;
            let reader = CandidateBus::new(&mut memory, &cart);
            let sampled = reader.cpu_read(address);
            if SaveLatch::save_address(target) {
                assert_eq!(
                    reader.fault.get(),
                    Some(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Read,
                    })
                );
            } else {
                assert_eq!(reader.fault.get(), None);
                if active {
                    assert_eq!(
                        sampled,
                        match target {
                            0x1234 => 0xa5,
                            0x211234 => 0xb6,
                            0x3fffff => 0xc7,
                            _ => unreachable!(),
                        }
                    );
                } else {
                    assert_eq!(sampled, cart.bytes[target as usize & (ROM_BYTES - 1)]);
                }
            }
        }
    }
}

pub(crate) fn contract() {
    identity();
    let template = Cartridge::new(image(&[0x4e71, 0x4e71])).unwrap();
    assert!(template.save_latch.is_some());
    template.validate_save_latch().unwrap();
    mapping(&template);
    guards(&template);
    sound_mapping(&template);
    crate::emulator::save_latch_tests::contract(template.bytes.to_vec());
}

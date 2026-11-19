use super::*;
use zeff_m68k::bus::FunctionCode;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & zeff_m68k::bus::BUS_ADDRESS_MASK & !1,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn main_register_byte_lanes_words_aliases_peek_and_unmatched_guards() {
    crate::hardware::bus::tmss::contract();
    crate::hardware::bus::bounded_odd::contract();
    crate::hardware::bus::bounded_switched::contract();
    or_bank_projection();
    banked_rom_controls();
    normal_slot_control_guards();
    unclaimed_write_completion();
    inserted_save_lanes();
    for profile in 0..3 {
        let cartridge = Cartridge::new(crate::cartridge::registers::tests::rom(profile)).unwrap();
        let rom = cartridge.bytes.clone();
        let mut bus = Bus::new(cartridge, Region::NtscUs);
        for address in [0x400000, 0x400002, 0x400004, 0x400006] {
            let alias = address ^ 2;
            for value in [0, 0x42, 0xff] {
                bus.write(access(address, ByteLanes::Both), 0xa500 | u16::from(value))
                    .unwrap();
                assert_eq!(
                    bus.read(access(alias, ByteLanes::Both)),
                    Ok(u16::from(value))
                );
                assert_eq!(
                    bus.read(access(alias, ByteLanes::Upper)),
                    Ok(u16::from(value) << 8)
                );
                assert_eq!(bus.peek(alias), Some(value));
                bus.write(access(address + 1, ByteLanes::Lower), 0x66)
                    .unwrap();
                assert_eq!(bus.peek(alias), Some(value));
                assert!(bus.read(access(address + 1, ByteLanes::Lower)).is_err());
                bus.write(access(address, ByteLanes::Upper), 0x3c00)
                    .unwrap();
                assert_eq!(bus.peek(alias), Some(0x3c));
                assert_eq!(bus.peek(alias + 1), None);
            }
        }
        for address in [0x400008, 0x407ffe, 0x40fffe] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                let before = bus.cartridge.clone();
                bus.write(access(address, lanes), 0xbeef).unwrap();
                assert_eq!(bus.cartridge, before);
                assert!(bus.read(access(address, lanes)).is_err());
            }
        }
        for address in [0x3ffffe, 0x410000] {
            assert!(bus.write(access(address, ByteLanes::Both), 0xff).is_ok());
        }
        assert_eq!(bus.cartridge.bytes, rom);
        assert!(bus.cartridge.save.is_none());
    }
}

fn or_bank_projection() {
    use zeff_m68k::timing::{ClockedBus, CpuClocks};
    let cart = crate::cartridge::registers::bank_tests::fixture();
    let mut bus = Bus::new(cart, Region::NtscUs);
    for address in [0x700001, 0x777777, 0x7fffff] {
        for bank in [0, 1, 2, 3, 0x10, 0x1f, 0x20, 0x80] {
            bus.write(access(address, ByteLanes::Lower), bank).unwrap();
            for target in [0x1000, 0x9000, 0x11000, 0xf9000] {
                let source = (target | (u32::from(bank as u8 & 0x3f) << 15)) as usize;
                let expected = bus.cartridge.bytes.get(source).copied();
                assert_eq!(bus.peek(target), expected);
                assert_eq!(bus.cartridge.read(target), expected);
                assert_eq!(
                    bus.cartridge.rom_source_offset(target),
                    expected.map(|_| source)
                );
                assert!(bus.cartridge.mode_sensitive_read(target));
                let read = bus.read(access(target, ByteLanes::Both));
                if let Some(value) = expected {
                    assert_eq!(read, Ok(u16::from(value) * 0x101));
                } else {
                    assert!(read.is_err());
                }
            }
            assert_eq!(bus.peek(0x100000), Some(0xff));
            assert_eq!(bus.peek(0xbfffff), None);
        }
    }
    for (address, lanes) in [
        (0x700000, ByteLanes::Upper),
        (0x777776, ByteLanes::Both),
        (0x600001, ByteLanes::Lower),
        (0x6fffff, ByteLanes::Lower),
    ] {
        let before = bus.clone();
        let clock = bus.cpu_clock;
        assert!(bus.write(access(address, lanes), 1).is_err());
        assert_eq!(bus, before);
        let outcome =
            ClockedBus::write_cycle(&mut bus, access(address, lanes), 1, CpuClocks(clock.0));
        assert!(outcome.result.is_err());
        assert_eq!(bus, before);
        let mut cpu = zeff_z80::Cpu::new();
        let mut schedule = crate::hardware::sound::schedule::SoundSchedule::default();
        let before_cpu = cpu.clone();
        let before_schedule = schedule.clone();
        let outcome = ClockedBus::write_cycle(
            &mut crate::hardware::bus::scheduled::ScheduledBus::new(
                &mut bus,
                &mut cpu,
                &mut schedule,
            ),
            access(address, lanes),
            1,
            clock,
        );
        assert!(outcome.result.is_err());
        assert_eq!(bus, before);
        assert_eq!(cpu, before_cpu);
        assert_eq!(schedule, before_schedule);
    }
    let before = bus.clone();
    assert!(matches!(
        ClockedBus::begin_reset_output(&mut bus, CpuClocks(0)),
        Ok(zeff_m68k::bus::BusResponse::Unsupported)
    ));
    assert_eq!(bus, before);
    for address in [0x600001, 0x600003, 0x600005, 0x700001, 0x7fffff] {
        assert!(bus.read(access(address, ByteLanes::Lower)).is_err());
        assert_eq!(bus.peek(address), None);
    }
}

fn banked_rom_controls() {
    let cartridge = crate::cartridge::ssf2::tests::cartridge(&[]);
    let mut bus = Bus::new(cartridge, Region::NtscUs);
    assert_eq!(bus.peek(0x80000), Some(1));
    assert_eq!(bus.read(access(0x80000, ByteLanes::Both)), Ok(0x0101));
    bus.write(access(0xa130f3, ByteLanes::Lower), 8).unwrap();
    assert_eq!(bus.read(access(0x80000, ByteLanes::Both)), Ok(0x0808));
    let before = bus.cartridge.clone();
    bus.write(access(0xa130f1, ByteLanes::Lower), 2).unwrap();
    assert_eq!(bus.cartridge, before);
    for (address, lanes, value) in [
        (0xa130f2, ByteLanes::Both, 9),
        (0xa130f2, ByteLanes::Upper, 0x900),
        (0xa130f1, ByteLanes::Lower, 8),
        (0xa130f3, ByteLanes::Lower, 10),
    ] {
        assert!(bus.write(access(address, lanes), value).is_err());
        assert_eq!(bus.cartridge, before);
    }
    assert!(bus.read(access(0xa130f3, ByteLanes::Lower)).is_err());
    assert_eq!(bus.peek(0x400000), None);
}

fn inserted_save_lanes() {
    let mut bus = Bus::new(
        crate::cartridge::registers::tests::inserted_save_fixture(),
        Region::Pal,
    );
    for mode in [1, 1, 0, 1] {
        bus.write(access(0xa130f1, ByteLanes::Lower), 0xab00 | mode)
            .unwrap();
        for (address, value) in [(0x200001, 0x12), (0x200201, 0x34), (0x2003ff, 0x56)] {
            bus.write(access(address, ByteLanes::Lower), value).unwrap();
            assert_eq!(
                bus.read(access(address, ByteLanes::Lower)).unwrap() as u8,
                if mode == 1 {
                    value as u8
                } else {
                    bus.cartridge.bytes[address as usize]
                }
            );
            assert_eq!(
                bus.peek(address),
                Some(if mode == 1 {
                    value as u8
                } else {
                    bus.cartridge.bytes[address as usize]
                })
            );
        }
        if mode == 0 {
            continue;
        }
        for address in [0x200000, 0x2003fe, 0x300000, 0x3ffffe] {
            for lanes in [ByteLanes::Upper, ByteLanes::Both, ByteLanes::Lower] {
                if lanes == ByteLanes::Lower && address < 0x300000 {
                    continue;
                }
                let before = bus.cartridge.clone();
                assert!(bus.write(access(address, lanes), 0xbeef).is_err());
                assert_eq!(bus.cartridge, before);
                assert!(bus.read(access(address, lanes)).is_err());
                assert_eq!(bus.cartridge, before);
            }
            assert_eq!(bus.peek(address), None);
            let mut fetch = access(address, ByteLanes::Both);
            fetch.kind = AccessKind::Prefetch;
            assert!(bus.read(fetch).is_err());
            assert_eq!(bus.prefetch, Prefetch::Empty);
            assert_eq!(
                crate::hardware::audio::trace::main_origin(address, &bus.cartridge).source,
                zeff_emu_common::audio_trace::MdTraceSource::Unknown
            );
        }
    }
    for (address, lanes, value) in [
        (0xa130f1, ByteLanes::Lower, 2),
        (0xa130f0, ByteLanes::Upper, 1),
        (0xa130f0, ByteLanes::Both, 1),
        (0x1a130f1, ByteLanes::Lower, 0),
        (0xa130f3, ByteLanes::Lower, 0),
    ] {
        let before = bus.cartridge.clone();
        assert!(bus.write(access(address, lanes), value).is_err());
        assert_eq!(bus.cartridge, before);
    }
}

fn normal_slot_control_guards() {
    use crate::cartridge::registers::LockOn::*;
    for mapping in [Empty, HalfMiB, MiB, TwoMiB] {
        let bytes = crate::cartridge::registers::tests::normal_rom_fixture(mapping);
        let mut cartridge = Cartridge::new(bytes).unwrap();
        cartridge.lock_on = Some(mapping);
        let mut bus = Bus::new(cartridge, Region::NtscJapan);
        let before = bus.cartridge.clone();
        for address in [0x200000, 0x280000, 0x300000, 0x380000] {
            let mut fetch = access(address, ByteLanes::Both);
            fetch.kind = AccessKind::Prefetch;
            let value = bus.read(fetch).unwrap();
            if mapping == Empty {
                assert_eq!(bus.prefetch, Prefetch::Empty);
                assert!(bus.read(access(0xa00000, ByteLanes::Both)).is_err());
            } else {
                assert_eq!(bus.prefetch, Prefetch::Full(value));
                assert_eq!(bus.read(access(0xa00000, ByteLanes::Both)), Ok(value));
                assert_eq!(bus.prefetch, Prefetch::Full(value));
            }
            use zeff_emu_common::audio_trace::MdTraceSource;
            let offset = match mapping {
                Empty => None,
                HalfMiB => Some(0x200000 + (address & 0x7ffff)),
                MiB => Some(0x200000 + (address & 0xfffff)),
                TwoMiB => Some(address),
                Patch => unreachable!(),
            };
            let expected = if mapping.mode_sensitive(address) {
                MdTraceSource::Unknown
            } else {
                offset.map_or(MdTraceSource::Unknown, |offset| {
                    MdTraceSource::EffectiveRom { offset }
                })
            };
            assert_eq!(
                crate::hardware::audio::trace::main_origin(address, &bus.cartridge).source,
                expected
            );
            assert_eq!(
                crate::hardware::audio::trace::sound_origin(
                    0x8000 | (address & 0x7fff) as u16,
                    (address >> 15) as u16,
                    &bus.cartridge
                )
                .source,
                expected
            );
        }
        for _ in 0..2 {
            bus.write(access(0xa130f1, ByteLanes::Lower), 0).unwrap();
            assert_eq!(bus.cartridge, before);
        }
        if mapping == MiB {
            patch_bus_projection(&mut bus);
            assert_eq!(bus.cartridge, before);
        } else {
            assert!(bus.write(access(0xa130f1, ByteLanes::Lower), 1).is_err());
            assert_eq!(bus.cartridge, before);
        }
        for (address, lanes, value) in [
            (0xa130f0, ByteLanes::Upper, 0),
            (0xa130f0, ByteLanes::Both, 0),
            (0xa130f0, ByteLanes::Both, 1),
            (0xa130f1, ByteLanes::Lower, 2),
            (0xa130f1, ByteLanes::Lower, 3),
            (0xa130f1, ByteLanes::Lower, 0x80),
            (0xa130f3, ByteLanes::Lower, 0),
            (0xa130ff, ByteLanes::Lower, 0),
            (0x01a130f1, ByteLanes::Lower, 0),
        ] {
            assert_eq!(
                bus.write(access(address, lanes), value),
                Err(BusError::UnsupportedAccess {
                    address: address & 0xfffffe,
                    direction: Direction::Write,
                })
            );
            assert_eq!(bus.cartridge, before);
        }
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            assert!(bus.read(access(0xa130f0, lanes)).is_err());
        }
        bus.cartridge.registers = Some(CartridgeRegisters::default());
        assert!(bus.write(access(0xa130f1, ByteLanes::Lower), 0).is_err());
        bus.cartridge.registers = None;
        bus.cartridge.lock_on = None;
        assert!(bus.write(access(0xa130f1, ByteLanes::Lower), 0).is_err());
        assert!(bus.cartridge.save.is_none());
    }
}

fn patch_bus_projection(bus: &mut Bus) {
    use zeff_emu_common::audio_trace::MdTraceSource;
    for mode in [1, 1, 0] {
        bus.write(access(0xa130f1, ByteLanes::Lower), mode).unwrap();
        for address in [0x2ffffe, 0x300000, 0x33fffe, 0x340000, 0x380000, 0x3ffffe] {
            let offset = if address < 0x300000 {
                address
            } else if mode == 1 {
                0x300000 + (address & 0x3ffff)
            } else {
                0x200000 + (address & 0xfffff)
            };
            let expected = u16::from_be_bytes(
                bus.cartridge.bytes[offset as usize..offset as usize + 2]
                    .try_into()
                    .unwrap(),
            );
            let mut fetch = access(address, ByteLanes::Both);
            fetch.kind = AccessKind::Prefetch;
            assert_eq!(bus.read(fetch), Ok(expected));
            assert_eq!(bus.prefetch, Prefetch::Full(expected));
            assert_eq!(bus.read(access(0xa00000, ByteLanes::Both)), Ok(expected));
            assert_eq!(bus.peek(address), Some((expected >> 8) as u8));
            let source = if address >= 0x300000 {
                MdTraceSource::Unknown
            } else {
                MdTraceSource::EffectiveRom { offset }
            };
            assert_eq!(
                crate::hardware::audio::trace::main_origin(address, &bus.cartridge).source,
                source
            );
            assert_eq!(
                crate::hardware::audio::trace::sound_origin(
                    0x8000 | (address & 0x7fff) as u16,
                    (address >> 15) as u16,
                    &bus.cartridge
                )
                .source,
                source
            );
        }
        for value in [2, 3, 0xff] {
            let before = bus.cartridge.clone();
            assert!(
                bus.write(access(0xa130f1, ByteLanes::Lower), value)
                    .is_err()
            );
            assert_eq!(bus.cartridge, before);
        }
    }
}

fn unclaimed_write_completion() {
    let mut rom = vec![0; 0x200];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    let mut bus = Bus::new(Cartridge::new(rom).unwrap(), Region::NtscUs);
    for address in [
        0x400000, 0x4e4c3e, 0x4e4c40, 0x4e4c42, 0x7ffffe, 0x800000, 0x9ffffe,
    ] {
        for lanes in [ByteLanes::Both, ByteLanes::Upper, ByteLanes::Lower] {
            let access = access(address + u32::from(lanes == ByteLanes::Lower), lanes);
            let before = (
                bus.cartridge.clone(),
                bus.ram.clone(),
                bus.input.clone(),
                bus.sound.clone(),
            );
            let clock = bus.cpu_clock;
            let ticks = bus.now.get();
            bus.prefetch = Prefetch::Full(0x1234);
            let response = bus.write_cycle(access, 0xa55a, clock);
            assert_eq!(response.wait_clocks, 0);
            assert_eq!(
                response.result,
                if address <= CARTRIDGE_UNUSED_END {
                    Ok(())
                } else {
                    Err(BusError::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    })
                }
            );
            assert_eq!(bus.cpu_clock.0, clock.0 + 4);
            assert_eq!(bus.now.get(), ticks + 28);
            assert_eq!(bus.prefetch, Prefetch::Empty);
            assert_eq!(
                (
                    bus.cartridge.clone(),
                    bus.ram.clone(),
                    bus.input.clone(),
                    bus.sound.clone()
                ),
                before
            );
            let response = bus.read_cycle(access, bus.cpu_clock);
            assert_eq!(
                response.result,
                Err(BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Read
                })
            );
            assert_eq!(bus.cpu_clock.0, clock.0 + 8);
        }
    }
}

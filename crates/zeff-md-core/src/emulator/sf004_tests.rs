use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, BusResponse, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn rom() -> Vec<u8> {
    let mut rom = vec![0; 0x200000];
    for (bank, page) in rom.chunks_mut(0x40000).enumerate() {
        page.fill(0x10 + bank as u8);
    }
    rom[..4].copy_from_slice(&0x00ff0100_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA GENESIS    ");
    rom[0x180..0x18e].copy_from_slice(b"SF-004        ");
    rom[0x1a0..0x1a8].copy_from_slice(&[0, 0, 0, 0, 0, 3, 0xff, 0xff]);
    rom[0x1b0..0x1bc].fill(b' ');
    rom[0x1f0..0x200].fill(b' ');
    rom[0x1f0] = b'U';
    rom[0x200..0x206].copy_from_slice(&[0x4e, 0xf9, 0x00, 0xff, 0x10, 0x00]);
    rom
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

fn mapping_matrix() {
    let rom = rom();
    let mut board = crate::cartridge::sf004::Sf004::new();
    assert!(board.save_bytes().unwrap().iter().all(|byte| *byte == 0xff));
    for bank in 0..8 {
        assert!(board.write(0xf01, bank << 4));
        assert_eq!(board.time_read(), bank << 4);
        for expanded in [false, true] {
            assert!(board.write(0xe01, if expanded { 0xc0 } else { 0x80 }));
            for address in [
                0, 0x3ffff, 0x40000, 0x7ffff, 0x100000, 0x13ffff, 0x140000, 0x1fffff,
            ] {
                let expected = if expanded {
                    (address < 0x140000)
                        .then_some(((usize::from(bank) << 18) + address as usize) & 0x1fffff)
                } else {
                    Some((usize::from(bank) << 18) | (address as usize & 0x3ffff))
                };
                assert_eq!(board.source_offset(address), expected);
                assert_eq!(
                    board.read(address, &rom),
                    expected.map(|offset| rom[offset])
                );
            }
            let before = board.clone();
            for address in [9, 0x10009, 0x100d01] {
                let byte = board.read(address, &rom);
                for value in [0, 0x40, 0xff] {
                    assert!(board.write(address, value));
                }
                assert_eq!(board, before);
                assert_eq!(board.read(address, &rom), byte);
            }
            for address in [0, 8, 0x1d01, 0x1e01, 0x1f01, 0xfdff, 0xfeff, 0xffff] {
                assert!(!board.write(address, 0x80));
            }
            for address in [0x140001, 0x1fffff] {
                assert_eq!(board.write(address, 0xff), !expanded);
            }
            assert_eq!(board, before);
        }
    }
    let before = board.clone();
    assert!(!board.write(0xe01, 0xa0));
    assert_eq!(board, before);
    assert!(!board.write(0x1d01, 0x80));
    assert!(board.write(0xd01, 0x80));
    assert_eq!(board.read(0x206001, &rom), Some(0xff));
    assert!(board.write(0x206001, 0x0e));
    assert!(board.write(0x206003, 0xa1));
    assert_eq!(board.read(0x216001, &rom), Some(0x0e));
    assert_eq!(board.read(0x206003, &rom), Some(0xa1));
    assert_eq!(board.read(0x206005, &rom), Some(0xff));
    assert_eq!(board.read(0x206000, &rom), None);
    assert!(!board.write(0x206000, 0xff));
    assert_eq!(board.save_bytes().unwrap().len(), 0x8000);
    assert!(board.write(0xe01, 0));
    let locked = board.clone();
    for (at, value) in [(0xd01, 0), (0xe01, 0xc0), (0xf01, 0x70)] {
        assert!(board.write(at, value));
        assert_eq!(board, locked);
    }
    board.power_on();
    assert_eq!(board.time_read(), 0);
    assert!(board.write(0xd01, 0x80));
    assert_eq!(board.read(0x206001, &rom), Some(0x0e));
}

pub(super) fn machine(concurrent_sound: bool) -> Emulator {
    let mut m = Emulator::new(
        rom(),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap();
    let mut code = Vec::new();
    for (at, value) in [
        (0x9, 0x40_u16),
        (0xd01, 0x80_u16),
        (0xf01, 0x63),
        (0xe01, 0xc0),
        (0x206001, 0x0e),
        (0x206003, 0xa1),
    ] {
        for word in [0x13fc, value, (at >> 16) as u16, at as u16] {
            code.extend_from_slice(&word.to_be_bytes());
        }
    }
    for word in [0x1039_u16, 0x00a1, 0x3001, 0x1239, 0x0020, 0x6001] {
        code.extend_from_slice(&word.to_be_bytes());
    }
    for (at, value) in [(0xe01, 0x80_u16), (0xf01, 0x10)] {
        for word in [0x13fc, value, (at >> 16) as u16, at as u16] {
            code.extend_from_slice(&word.to_be_bytes());
        }
    }
    let end = 0xff1000 + code.len() as u32;
    code.extend_from_slice(&[0x60, 0xfe]);
    m.bus.ram[0x1000..0x1000 + code.len()].copy_from_slice(&code);
    for _ in 0..32 {
        if m.registers().pc == end {
            break;
        }
        m.step_instruction().unwrap();
    }
    assert_eq!(m.registers().pc, end);
    assert_eq!(m.registers().d[0] & 255, 0x60);
    assert_eq!(m.registers().d[1] & 255, 0x0e);
    assert_eq!(m.bus.cartridge.sf004.as_ref().unwrap().time_read(), 0x10);
    assert_eq!(m.peek(0x1234), Some(0x11));
    assert_eq!(m.peek(0x206001), Some(0x0e));
    m
}

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
    assert_eq!(machine.sound_fault(), None);
}

fn pending_futures() {
    for phase in 0..3 {
        let mut source = machine(true);
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 3;
        source.bus.sound.ram[..4].copy_from_slice(&[0x3a, 1, 0x80, 0x76]);
        for _ in 0..600 {
            advance(&mut source, 1);
            if source
                .sound_schedule
                .pending
                .as_ref()
                .and_then(|p| p.bank_access)
                .is_some_and(|access| access.address == 0x18001)
            {
                break;
            }
        }
        assert_eq!(
            source
                .sound_schedule
                .pending
                .as_ref()
                .unwrap()
                .bank_access
                .unwrap()
                .address,
            0x18001
        );
        let deadline = source.sound_deadline().unwrap().get();
        let remaining = deadline - source.bus.now.get();
        let ticks = match phase {
            0 => 0,
            1 => 1,
            _ => remaining - 1,
        };
        advance(&mut source, ticks);
        assert!(
            source
                .bus
                .cartridge
                .sf004
                .as_mut()
                .unwrap()
                .write(0xf01, 0x20)
        );
        let raw = source.encode_state().unwrap();
        let front = source.encode_frontend_state().unwrap();
        let fresh = || {
            Emulator::new(
                rom(),
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound: true,
                },
            )
            .unwrap()
        };
        let mut frontend = fresh();
        frontend.decode_frontend_state(&front).unwrap();
        let mut reference = source.clone();
        reference.decode_state(&raw).unwrap();
        let mut restored = fresh();
        restored.decode_state(&raw).unwrap();
        assert_eq!(frontend.encode_frontend_state().unwrap(), front);
        assert_eq!(restored.encode_state().unwrap(), raw);
        for ticks in [deadline - source.bus.now.get() - 1, 1, 200] {
            for machine in [&mut source, &mut frontend, &mut reference, &mut restored] {
                advance(machine, ticks);
            }
            assert_eq!(
                source.encode_state().unwrap(),
                frontend.encode_state().unwrap()
            );
            assert_eq!(
                source.encode_state().unwrap(),
                reference.encode_state().unwrap()
            );
            assert_eq!(
                reference.encode_state().unwrap(),
                restored.encode_state().unwrap()
            );
            assert_eq!(
                source.encode_frontend_state().unwrap(),
                frontend.encode_frontend_state().unwrap()
            );
            assert_eq!(
                reference.encode_frontend_state().unwrap(),
                restored.encode_frontend_state().unwrap()
            );
            assert_eq!(source.framebuffer(), restored.framebuffer());
            for (left, right) in [
                (&mut source, &mut frontend),
                (&mut reference, &mut restored),
            ] {
                let a = left.drain_audio_samples();
                let b = right.drain_audio_samples();
                assert_eq!(a.len(), b.len());
                assert!(a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits()));
            }
        }
        assert_eq!(source.sound_cpu.regs().a, 0x12);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf004_mapping_provenance_and_state_boundaries() {
    mapping_matrix();
    pending_futures();
    for concurrent in [false, true] {
        let mut m = machine(concurrent);
        SoundSchedule::assert_sf004_boundaries(&m.bus.cartridge);
        crate::hardware::vdp::VdpDmaSource::assert_sf004_boundaries(&m.bus.cartridge);
        for which in 0..4 {
            let mut bad = access(0x1234, ByteLanes::Both);
            match which {
                0 => bad.function_code = FunctionCode::CpuSpace,
                1 => bad.kind = AccessKind::InterruptAcknowledge,
                2 => bad.logical_address += 2,
                _ => bad.bus_address += 1,
            }
            let mut probe = m.clone();
            assert!(probe.bus.read_cycle(bad, probe.cpu.clock()).result.is_err());
        }
        for mut bad in [
            access(0xd00, ByteLanes::Both),
            access(0xd00, ByteLanes::Upper),
            access(0x1d01, ByteLanes::Lower),
            access(0x206000, ByteLanes::Upper),
            access(0xa130f1, ByteLanes::Lower),
        ] {
            let before = m.clone();
            let start = m.cpu.clock();
            let result = if concurrent {
                ScheduledBus::new(&mut m.bus, &mut m.sound_cpu, &mut m.sound_schedule)
                    .write_cycle(bad, 0x80, start)
            } else {
                m.bus.write_cycle(bad, 0x80, start)
            };
            assert!(result.result.is_err());
            assert_eq!(m, before);
            bad = access(0xe01, ByteLanes::Lower);
            bad.kind = AccessKind::Prefetch;
            let before = m.clone();
            assert!(m.bus.write_cycle(bad, 0x80, m.cpu.clock()).result.is_err());
            assert_eq!(m, before);
        }
        let before = m.clone();
        assert!(m.reset_cpu().is_err());
        assert_eq!(m, before);
        assert!(m.check_warm_reset().is_err());
        assert_eq!(m, before);
        assert_eq!(
            m.bus.begin_reset_output(m.cpu.clock()).unwrap(),
            BusResponse::Unsupported
        );
        assert_eq!(m, before);
        let mut scheduled = ScheduledBus::new(&mut m.bus, &mut m.sound_cpu, &mut m.sound_schedule);
        let start = scheduled.bus.cpu_clock;
        assert_eq!(
            scheduled.begin_reset_output(start).unwrap(),
            BusResponse::Unsupported
        );
        assert_eq!(m, before);
        assert_eq!(m.dump_save_ram().unwrap().len(), 0x8000);
        let raw = m.encode_state().unwrap();
        assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
        let front = m.encode_frontend_state().unwrap();
        let mut fresh = Emulator::new(
            rom(),
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: concurrent,
            },
        )
        .unwrap();
        fresh.decode_frontend_state(&front).unwrap();
        assert_eq!(fresh.encode_frontend_state().unwrap(), front);
        for _ in 0..32 {
            m.step_instruction().unwrap();
            fresh.step_instruction().unwrap();
            assert_eq!(
                m.encode_frontend_state().unwrap(),
                fresh.encode_frontend_state().unwrap()
            );
        }
        let mut reader = zeff_emu_common::save_state::StateReader::new(&raw);
        reader.set_position(52);
        for _ in 0..3 {
            reader
                .read_slice(crate::save_state::MAX_STATE_BYTES)
                .unwrap();
        }
        reader.read_bool().unwrap();
        for _ in 0..5 {
            reader
                .read_slice(crate::save_state::MAX_STATE_BYTES)
                .unwrap();
        }
        let footer_at = reader.position();
        let footer = reader
            .read_slice(crate::save_state::MAX_STATE_BYTES)
            .unwrap();
        assert_eq!(&footer[1..5], b"SF04");
        assert_eq!(
            reader
                .read_slice(crate::save_state::MAX_STATE_BYTES)
                .unwrap(),
            &[0]
        );
        assert_eq!(
            reader
                .read_slice(crate::save_state::MAX_STATE_BYTES)
                .unwrap(),
            &[0]
        );
        assert_eq!(
            reader
                .read_slice(crate::save_state::MAX_STATE_BYTES)
                .unwrap(),
            &[0]
        );
        assert!(reader.is_exhausted());
        let mut bad = raw.clone();
        bad[footer_at + 4 + 5] = 8;
        let before = fresh.encode_frontend_state().unwrap();
        assert!(fresh.decode_state(&bad).is_err());
        assert_eq!(fresh.encode_frontend_state().unwrap(), before);
        let mut undefined = raw.clone();
        undefined[footer_at + 4 + 1 + 12 + 0x8000] &= !1;
        assert!(fresh.decode_state(&undefined).is_err());
        assert_eq!(fresh.encode_frontend_state().unwrap(), before);
        let mut partial = undefined.clone();
        partial[footer_at + 4 + 1 + 12] = 0;
        fresh.decode_state(&partial).unwrap();
        assert_eq!(fresh.encode_state().unwrap(), partial);
        assert_eq!(fresh.peek(0x200001), None);
        assert!(fresh.dump_save_ram().is_none());
        fresh.reset();
        assert!(
            fresh
                .bus
                .cartridge
                .sf004
                .as_mut()
                .unwrap()
                .write(0xd01, 0x80)
        );
        assert_eq!(fresh.peek(0x200001), None);
        assert!(fresh.dump_save_ram().is_none());
        fresh.decode_frontend_state(&before).unwrap();
        assert_eq!(fresh.encode_frontend_state().unwrap(), before);

        assert!(fresh.load_save_ram(&[0; 32]).is_err());
        assert_eq!(fresh.encode_frontend_state().unwrap(), before);
        fresh.load_save_ram(&vec![0x55; 0x8000]).unwrap();
        assert_eq!(fresh.dump_save_ram(), Some(vec![0x55; 0x8000]));
        fresh.reset();
        assert!(fresh.dump_save_ram().unwrap().iter().all(|v| *v == 0x55));
        let mut generic_rom = rom();
        generic_rom[0x180..0x18e].fill(b' ');
        let mut generic = Emulator::new(generic_rom, LoadConfig::default()).unwrap();
        let current = generic.encode_state().unwrap();

        generic.decode_state(&current).unwrap();
        assert_eq!((&generic.encode_state().unwrap()), (&current));
    }
}

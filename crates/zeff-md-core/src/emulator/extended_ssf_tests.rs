use super::*;
use crate::cartridge::extended_ssf::ExtendedSsf;
use zeff_m68k::bus::{AccessKind, BusAccess, BusResponse, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn rom() -> Vec<u8> {
    let mut r = vec![0; 40 * 0x80000];
    for (page, bytes) in r.chunks_mut(0x80000).enumerate() {
        bytes.fill(page as u8);
    }
    r[..4].copy_from_slice(&0x00fff000_u32.to_be_bytes());
    r[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    r[0x100..0x110].fill(b' ');
    r[0x100..0x108].copy_from_slice(b"SEGA SSF");
    r[0x1b0..0x1bc].fill(0);
    r[0x1f0..0x200].fill(b' ');
    r[0x1f0] = b'U';
    r[0x200..0x206].copy_from_slice(&[0x4e, 0xf9, 0, 0xff, 0x20, 0]);
    r
}
fn fresh(concurrent_sound: bool) -> Emulator {
    let bytes = rom();
    assert!(Emulator::supports_cartridge_size(&bytes));
    Emulator::new(
        bytes,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap()
}
fn machine(concurrent: bool) -> Emulator {
    let mut m = fresh(concurrent);
    let words = [
        0x13fc, 37, 0x00a1, 0x30f0, 0x2039, 0, 0x1000, 0x13fc, 38, 0x00a1, 0x30f3, 0x33fc, 0xab27,
        0x00a1, 0x30f4, 0x1239, 0x0008, 0x1000, 0x1439, 0x0010, 0x1000, 0x60fe,
    ];
    for (i, word) in words.into_iter().enumerate() {
        m.bus.ram[0x2000 + i * 2..0x2002 + i * 2].copy_from_slice(&u16::to_be_bytes(word));
    }
    for _ in 0..12 {
        if m.registers().pc == 0xff202a {
            break;
        }
        m.step_instruction().unwrap();
    }
    assert_eq!(m.registers().pc, 0xff202a);
    assert_eq!(m.registers().d[0], 0x25252525);
    assert_eq!(m.registers().d[1] & 255, 38);
    assert_eq!(m.registers().d[2] & 255, 39);
    m
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
fn advance(m: &mut Emulator, ticks: u64) {
    ScheduledBus::new(&mut m.bus, &mut m.sound_cpu, &mut m.sound_schedule)
        .advance_machine(ticks)
        .unwrap();
    assert_eq!(m.sound_fault(), None);
}
fn geometry() {
    for (size, mask) in [
        (0x800001, 0x1f_u8),
        (16_usize * 1024 * 1024, 0x1f),
        (0x1000001, 0x3f),
        (32_021_508, 0x3f),
        (0x2000000, 0x3f),
        (0x2000001, 0x7f),
        (0x4000000, 0x7f),
        (0x4000001, 0xff),
        (84 * 1024 * 1024, 0xff),
        (128 * 1024 * 1024, 0xff),
    ] {
        let mut board = ExtendedSsf::new(size);
        for slot in 0..8 {
            for page in [0, 31, 32, 37, 61, 127, 128, 167, 255] {
                board.control(0xa130f0 + slot * 2, page);
                for within in [0, 3, 0x7fffe, 0x7ffff] {
                    let at = usize::from(page & mask) * 0x80000 + within as usize;
                    assert_eq!(
                        board.source_offset(slot * 0x80000 + within),
                        (at < size).then_some(at)
                    );
                }
            }
        }
        assert_eq!(board.source_offset(0x400000), None);
        let mut writer = zeff_emu_common::save_state::StateWriter::new();
        board.write_state(&mut writer);
        let bytes = writer.into_bytes();
        assert_eq!(&bytes[..4], b"ESF1");
        assert_eq!(bytes.len(), 12);
        let mut restored = ExtendedSsf::new(size);
        restored
            .read_state(&mut zeff_emu_common::save_state::StateReader::new(&bytes))
            .unwrap();
        assert_eq!(board, restored);
        board.power_on();
        for slot in 0..8 {
            assert_eq!(
                board.source_offset(slot * 0x80000),
                Some(slot as usize * 0x80000)
            );
        }
    }
}
fn futures(mut source: Emulator, pending: bool) {
    if pending {
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 0;
        source.bus.sound.ram[..4].copy_from_slice(&[0x3a, 0, 0x90, 0x76]);
        for _ in 0..600 {
            advance(&mut source, 1);
            if source
                .sound_schedule
                .pending
                .as_ref()
                .and_then(|p| p.bank_access)
                .is_some_and(|a| a.address == 0x1000)
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
            0x1000
        );
        source
            .bus
            .cartridge
            .extended_ssf
            .as_mut()
            .unwrap()
            .control(0xa130f0, 38);
    }
    let raw = source.encode_state().unwrap();
    let front = source.encode_frontend_state().unwrap();
    let mut reader = zeff_emu_common::save_state::StateReader::new(&raw);
    reader.set_position(52);
    for _ in 0..3 {
        reader
            .read_slice(crate::save_state::MAX_STATE_BYTES)
            .unwrap();
    }
    reader.read_bool().unwrap();
    for _ in 0..6 {
        reader
            .read_slice(crate::save_state::MAX_STATE_BYTES)
            .unwrap();
    }
    let footer_at = reader.position();
    let footer = reader
        .read_slice(crate::save_state::MAX_STATE_BYTES)
        .unwrap();
    assert_eq!(&footer[..5], &[1, b'E', b'S', b'F', b'1']);
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
    let mut frontend = fresh(source.concurrent_sound);
    frontend.decode_frontend_state(&front).unwrap();
    let mut reference = source.clone();
    reference.decode_state(&raw).unwrap();
    let mut restored = fresh(source.concurrent_sound);
    restored.decode_state(&raw).unwrap();
    let mut legacy_reference = source.clone();
    legacy_reference.decode_state(&raw).unwrap();
    assert_eq!(frontend.encode_frontend_state().unwrap(), front);
    assert_eq!(
        restored.encode_state().unwrap(),
        legacy_reference.encode_state().unwrap()
    );
    assert_eq!((&restored.encode_state().unwrap()), (&raw));
    let before = restored.encode_frontend_state().unwrap();
    let mut bad = raw.clone();
    bad[footer_at + 4 + 5] = 0x80;
    assert!(restored.decode_state(&bad).is_err());
    assert_eq!(restored.encode_frontend_state().unwrap(), before);

    for _ in 0..16 {
        if pending {
            for m in [
                &mut source,
                &mut frontend,
                &mut reference,
                &mut restored,
                &mut legacy_reference,
            ] {
                advance(m, 30);
            }
        } else {
            for m in [
                &mut source,
                &mut frontend,
                &mut reference,
                &mut restored,
                &mut legacy_reference,
            ] {
                m.step_instruction().unwrap();
            }
        }
        assert_eq!(
            source.encode_state().unwrap(),
            frontend.encode_state().unwrap()
        );
        assert_eq!(
            legacy_reference.encode_state().unwrap(),
            restored.encode_state().unwrap()
        );
        assert_eq!(
            source.encode_state().unwrap(),
            reference.encode_state().unwrap()
        );
        assert_eq!(
            (&reference.encode_state().unwrap()),
            (&restored.encode_state().unwrap())
        );
        assert_eq!(legacy_reference.framebuffer(), restored.framebuffer());
        assert_eq!(
            source.encode_frontend_state().unwrap(),
            frontend.encode_frontend_state().unwrap()
        );
        assert_eq!(
            legacy_reference.encode_frontend_state().unwrap(),
            restored.encode_frontend_state().unwrap()
        );
        let source_samples = source.drain_audio_samples();
        let frontend_samples = frontend.drain_audio_samples();
        let reference_samples = reference.drain_audio_samples();
        let restored_samples = restored.drain_audio_samples();
        let legacy_samples = legacy_reference.drain_audio_samples();
        for (x, y) in [
            (&source_samples, &frontend_samples),
            (&reference_samples, &restored_samples),
            (&legacy_samples, &restored_samples),
        ] {
            assert_eq!(x.len(), y.len());
            assert!(x.iter().zip(y).all(|(x, y)| x.to_bits() == y.to_bits()));
        }
    }
    if pending {
        assert_eq!(source.sound_cpu.regs().a, 38);
    }
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn extended_windows_restore_and_publication() {
    geometry();
    let mut boot = rom();
    boot.truncate(6 * 1024 * 1024);
    let mapped_end = u32::try_from(crate::constants::MAX_CARTRIDGE_SIZE).unwrap();
    let config = LoadConfig::default();
    let valid_pc = mapped_end - 4;
    boot[4..8].copy_from_slice(&valid_pc.to_be_bytes());
    assert!(Emulator::new(boot.clone(), config).is_ok());
    for pc in [mapped_end - 2, mapped_end] {
        boot[4..8].copy_from_slice(&pc.to_be_bytes());
        assert!(matches!(
            Emulator::new(boot.clone(), config),
            Err(LoadError::ResetProgramCounter(rejected)) if rejected == pc
        ));
    }
    for concurrent in [false, true] {
        let m = machine(concurrent);
        for address in [0x1000, 0x81000] {
            assert_eq!(
                crate::hardware::audio::trace::main_origin(address, &m.bus.cartridge).source,
                zeff_emu_common::audio_trace::MdTraceSource::Unknown
            );
        }
        let mut user = m.clone();
        let mut transfer = access(0xa130f0, ByteLanes::Upper);
        transfer.function_code = FunctionCode::UserData;
        let start = user.cpu.clock();
        user.bus
            .write_cycle(transfer, 33 << 8, start)
            .result
            .unwrap();
        assert_eq!(user.peek(0x1000), Some(33));
        for defect in 0..2 {
            let mut bad = access(0xa130f0, ByteLanes::Upper);
            if defect == 0 {
                bad.function_code = FunctionCode::CpuSpace;
            } else {
                bad.logical_address += 2;
            }
            let mut probe = m.clone();
            let before = probe.clone();
            let start = probe.cpu.clock();
            let result = if concurrent {
                ScheduledBus::new(
                    &mut probe.bus,
                    &mut probe.sound_cpu,
                    &mut probe.sound_schedule,
                )
                .write_cycle(bad, 0x2500, start)
            } else {
                probe.bus.write_cycle(bad, 0x2500, start)
            };
            assert!(result.result.is_err());
            assert_eq!(probe, before);
        }
        if concurrent {
            crate::hardware::vdp::VdpDmaSource::assert_extended_windows(&m.bus.cartridge);
            SoundSchedule::assert_extended_windows(&m.bus.cartridge);
        }
        for mut bad in [
            access(0xa130ee, ByteLanes::Both),
            access(0x400000, ByteLanes::Both),
            access(0xa130f0, ByteLanes::Both),
        ] {
            if bad.bus_address == 0xa130f0 {
                bad.kind = AccessKind::Prefetch;
            }
            let mut probe = m.clone();
            let before = probe.clone();
            let start = probe.cpu.clock();
            let result = if concurrent {
                ScheduledBus::new(
                    &mut probe.bus,
                    &mut probe.sound_cpu,
                    &mut probe.sound_schedule,
                )
                .write_cycle(bad, 0x2525, start)
            } else {
                probe.bus.write_cycle(bad, 0x2525, start)
            };
            assert!(result.result.is_err());
            assert_eq!(probe, before);
        }
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            let mut probe = m.clone();
            let board = probe.bus.cartridge.extended_ssf;
            let address = 0x1000 + u32::from(lanes == ByteLanes::Lower);
            let start = probe.cpu.clock();
            let result = if concurrent {
                ScheduledBus::new(
                    &mut probe.bus,
                    &mut probe.sound_cpu,
                    &mut probe.sound_schedule,
                )
                .write_cycle(access(address, lanes), 0x1234, start)
            } else {
                probe.bus.write_cycle(access(address, lanes), 0x1234, start)
            };
            result.result.unwrap();
            assert_eq!(probe.bus.cartridge.extended_ssf, board);
            assert_eq!(probe.peek(0x1000), Some(37));
        }
        let mut probe = m.clone();
        let before = probe.clone();
        assert!(probe.reset_cpu().is_err());
        assert!(probe.check_warm_reset().is_err());
        assert_eq!(
            probe.bus.begin_reset_output(probe.cpu.clock()).unwrap(),
            BusResponse::Unsupported
        );
        assert_eq!(probe, before);
        let start = probe.cpu.clock();
        assert_eq!(
            ScheduledBus::new(
                &mut probe.bus,
                &mut probe.sound_cpu,
                &mut probe.sound_schedule
            )
            .begin_reset_output(start)
            .unwrap(),
            BusResponse::Unsupported
        );
        assert_eq!(probe, before);
        probe.reset();
        assert_eq!(probe.peek(0x1000), Some(0));
        futures(m, false);
    }
    futures(machine(true), true);
    let mut sf004 = super::sf004_tests::machine(true);
    let raw = sf004.encode_state().unwrap();

    sf004.decode_state(&raw).unwrap();
    assert_eq!((&sf004.encode_state().unwrap()), (&raw));
}

use super::*;

mod legacy;

#[test]
fn legacy_machine_states_restore_missing_hardware_defaults_and_continue() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for version in MIN_STATE_VERSION..=crate::save_state::STATE_VERSION_WITH_INTERLACE {
                let mut source = machine(region, concurrent);
                for _ in 0..17 {
                    source.step_instruction().unwrap();
                }
                let current = source.encode_state().unwrap();
                let legacy = legacy::progressive_state(&current, version);
                if version < crate::save_state::STATE_VERSION_WITH_INTERLACE {
                    assert!(legacy.len() < current.len() - 307_200);
                } else {
                    assert_eq!(legacy.len(), current.len() - 5);
                }
                source.decode_state(&legacy).unwrap();
                let mut target = source.clone();
                assert!(target.bus.input.write_serial(0, 0x55));
                assert!(target.bus.input.write_serial(2, 0xc8));
                target.decode_state(&legacy).unwrap();
                assert_eq!(
                    target.encode_state().unwrap(),
                    source.encode_state().unwrap()
                );
                for _ in 0..32 {
                    assert_eq!(target.step_instruction(), source.step_instruction());
                }
                assert_eq!(
                    target.encode_state().unwrap(),
                    source.encode_state().unwrap()
                );
            }
        }
    }
}

#[test]
fn future_video_field_identity_is_rejected_atomically_by_bus_and_outer_state() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = machine(region, false);
        while source.timing_snapshot().now().get() < 72 * crate::constants::VDP_LINE_TICKS {
            source.step_instruction().unwrap();
        }
        let saved = source.encode_state().unwrap();
        let (bus_at, video_at) = legacy::section_offsets(&saved);
        let mut reader = StateReader::new(&saved[video_at..]);
        reader.read_u16().unwrap();
        let height = reader.read_u16().unwrap();
        let rows = reader.read_u16().unwrap();
        assert!(rows > 0 && rows < height);
        let field_at = video_at + 15 + crate::constants::FRAMEBUFFER_LEN;
        assert_eq!(saved[field_at], 1);
        let position = source.bus.vdp.field_position(region).unwrap();
        let mut malformed = saved.clone();
        malformed[field_at + 1..field_at + 9].copy_from_slice(&(position.id + 1).to_le_bytes());
        let mut target = source.clone();
        assert!(
            target
                .decode_state(&malformed)
                .unwrap_err()
                .to_string()
                .contains("ahead of its calendar")
        );
        assert_eq!(target, source);
        let bus_length =
            u32::from_le_bytes(malformed[bus_at - 4..bus_at].try_into().unwrap()) as usize;
        let mut bus = source.bus.clone();
        assert!(
            bus.read_state(
                &mut StateReader::new(&malformed[bus_at..bus_at + bus_length]),
                STATE_VERSION
            )
            .unwrap_err()
            .to_string()
            .contains("ahead of its calendar")
        );
        assert_eq!(bus, source.bus);
    }
}

fn machine(region: Region, concurrent_sound: bool) -> Emulator {
    let mut bytes = vec![0; 0x200];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in [0x7080_u16, 0x7201, 0x60fa, 0x4e71].into_iter().enumerate() {
        bytes[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        bytes,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

#[test]
fn complete_machine_state_restores_and_continues() {
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for concurrent in [false, true] {
            let mut source = machine(region, concurrent);
            for _ in 0..17 {
                source.step_instruction().unwrap();
            }
            let saved = source.encode_state().unwrap();
            let mut target = source.clone();
            for _ in 0..11 {
                target.step_instruction().unwrap();
            }
            target.decode_state(&saved).unwrap();
            assert_eq!(target.encode_state().unwrap(), saved);
            source.set_sample_rate(source.sample_rate());
            assert_eq!(source.encode_state().unwrap(), saved);
            for _ in 0..512 {
                assert_eq!(
                    target.step_instruction().unwrap(),
                    source.step_instruction().unwrap()
                );
            }
            assert_eq!(
                target.encode_state().unwrap(),
                source.encode_state().unwrap()
            );
            assert_eq!(target.framebuffer(), source.framebuffer());
            let samples = source.drain_audio_samples();
            assert!(!samples.is_empty());
            assert_eq!(target.drain_audio_samples(), samples);
        }
    }
}

#[test]
fn region_tags_preserve_existing_ids_and_reject_other_ntsc_console() {
    let mut target = machine(Region::NtscJapan, true);
    let before = target.clone();
    for (region, tag) in [
        (Region::NtscUs, 0),
        (Region::Pal, 1),
        (Region::NtscJapan, 2),
    ] {
        let saved = machine(region, true).encode_state().unwrap();
        assert_eq!(saved[48], tag);
        if region == Region::NtscJapan {
            target.decode_state(&saved).unwrap();
            assert_eq!(target.encode_state().unwrap(), saved);
        } else {
            assert!(target.decode_state(&saved).is_err());
            assert_eq!(target, before);
        }
    }
}

fn active(region: Region, stopped: bool) -> Emulator {
    let mut machine = super::super::cpu_continuation_tests::machine(region, stopped);
    for _ in 0..20_000 {
        if machine.sound_cpu.cycles() >= 400 {
            return machine;
        }
        machine.step_instruction().unwrap();
    }
    panic!("sound program did not start");
}

#[test]
fn complete_state_preserves_pending_sound_and_nonzero_pcm() {
    for region in [Region::NtscUs, Region::Pal] {
        for stopped in [false, true] {
            let mut source = active(region, stopped);
            let mut pending = false;
            let mut samples = 0;
            for _ in 0..8 {
                pending |= source.sound_schedule.pending.is_some();
                let saved = source.encode_state().unwrap();
                let mut target = source.clone();
                for _ in 0..13 {
                    target.step_instruction().unwrap();
                }
                target.decode_state(&saved).unwrap();
                assert_eq!(target.encode_state().unwrap(), saved);
                source.set_sample_rate(source.sample_rate());
                for _ in 0..1021 {
                    assert_eq!(
                        target.step_instruction().unwrap(),
                        source.step_instruction().unwrap()
                    );
                }
                assert_eq!(
                    target.encode_state().unwrap(),
                    source.encode_state().unwrap()
                );
                assert_eq!(target.framebuffer(), source.framebuffer());
                let expected = source.drain_audio_samples();
                assert_eq!(target.drain_audio_samples(), expected);
                for frame in expected.as_chunks::<2>().0 {
                    assert_eq!(*frame, [8128.0 / 32768.0, 0.0]);
                }
                samples += expected.len();
            }
            assert!(
                pending && samples > 0,
                "{region:?} stopped={stopped} pending={pending} samples={samples}"
            );
        }
    }
}

#[test]
fn state_identity_bounds_and_late_errors_preserve_live_machine() {
    let source = active(Region::NtscUs, false);
    let saved = source.encode_state().unwrap();
    let mut target = source.clone();
    for _ in 0..23 {
        target.step_instruction().unwrap();
    }
    let before = target.clone();
    let mut rejects = vec![
        saved[..12].to_vec(),
        saved[..52].to_vec(),
        vec![0; MAX_STATE_BYTES + 1],
    ];
    for length in saved.len() - 24..saved.len() {
        rejects.push(saved[..length].to_vec());
    }
    for (offset, value) in [
        (0, b'X'),
        (8, 0),
        (8, (STATE_VERSION + 1) as u8),
        (12, saved[12] ^ 1),
        (48, 1),
        (49, 0),
        (50, 1),
        (51, 1),
        (saved.len() - 1, 2),
    ] {
        let mut changed = saved.clone();
        changed[offset] = value;
        rejects.push(changed);
    }
    let mut extra = saved.clone();
    extra.push(0);
    rejects.push(extra);
    for bad in rejects {
        assert!(target.decode_state(&bad).is_err());
        assert_eq!(target, before);
    }
    let pal = active(Region::Pal, false);
    assert!(target.decode_state(&pal.encode_state().unwrap()).is_err());
    assert_eq!(target, before);
    let serial = machine(Region::NtscUs, false);
    assert!(
        target
            .decode_state(&serial.encode_state().unwrap())
            .is_err()
    );
    assert_eq!(target, before);
}

#[test]
fn malformed_pending_fm_stage_preserves_the_live_machine() {
    let source = active(Region::NtscUs, false);
    let mut saved = source.encode_state().unwrap();
    let mut reader = StateReader::new(&saved);
    reader.set_position(52);
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    let bus_start = reader.position() + 4;
    let bus = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let mut reader = StateReader::new(bus);
    reader.set_position(25);
    let ownership = usize::from(reader.read_bool().unwrap());
    reader.set_position(26 + ownership + crate::constants::WORK_RAM_SIZE);
    for _ in 0..4 {
        reader.read_slice(MAX_STATE_BYTES).unwrap();
    }
    let audio_start = bus_start + reader.position() + 4;
    let audio = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let mut reader = StateReader::new(audio);
    crate::hardware::audio::Psg::read_state(&mut reader).unwrap();
    let first_desired_key = audio_start + reader.position() + 15;
    saved[first_desired_key] = 2;
    let mut target = source.clone();
    for _ in 0..17 {
        target.step_instruction().unwrap();
    }
    let before = target.clone();
    assert!(target.decode_state(&saved).is_err());
    assert_eq!(target, before);
}

#[test]
fn runtime_audio_policy_is_retained_and_excluded_from_state() {
    let mut source = active(Region::NtscUs, false);
    let mut target = source.clone();
    source.set_sample_rate(44_100);
    target.set_sample_rate(96_000);
    target.set_apu_channel_mutes([true; 4]);
    target.set_fm_channel_mutes([true; 6]);
    target.set_apu_sample_generation_enabled(false);
    for _ in 0..512 {
        source.step_instruction().unwrap();
        target.step_instruction().unwrap();
    }
    let saved = source.encode_state().unwrap();
    assert_eq!(saved, target.encode_state().unwrap());
    target.decode_state(&saved).unwrap();
    assert_eq!(target.sample_rate(), 96_000);
    for _ in 0..1021 {
        target.step_instruction().unwrap();
    }
    assert!(target.drain_audio_samples().is_empty());
    target.set_apu_sample_generation_enabled(true);
    for _ in 0..512 {
        target.step_instruction().unwrap();
    }
    let samples = target.drain_audio_samples();
    assert!(!samples.is_empty() && samples.iter().all(|sample| *sample == 0.0));
}

#[test]
fn configuration_identity_and_clock_sections_are_checked_independently() {
    let source = active(Region::NtscUs, false);
    let saved = source.encode_state().unwrap();
    for (field, expected) in [
        (0, "region"),
        (1, "scheduling"),
        (2, "controller"),
        (3, "ROM"),
    ] {
        let mut target = source.clone();
        match field {
            0 => target.bus.region = Region::Pal,
            1 => target.concurrent_sound = false,
            2 => target
                .input_mut()
                .set_controller_kind(ControllerPort::Two, ControllerKind::SixButton),
            3 => {
                let mut rom = target.bus.cartridge.bytes.to_vec();
                rom[0x100] ^= 1;
                target.bus.cartridge = crate::cartridge::Cartridge::new(rom).unwrap();
            }
            _ => unreachable!(),
        }
        let before = target.clone();
        assert!(
            target
                .decode_state(&saved)
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
        assert_eq!(target, before);
    }
    let mut cursor = StateReader::new(&saved);
    cursor.set_position(52);
    cursor.read_slice(MAX_STATE_BYTES).unwrap();
    let bus_length = cursor.position();
    let mut rejects = Vec::new();
    for offset in [52, bus_length] {
        let mut bad = saved.clone();
        bad[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        rejects.push(bad);
    }
    for offset in [bus_length + 4, bus_length + 12, bus_length + 20] {
        let mut bad = saved.clone();
        bad[offset] ^= 0xff;
        rejects.push(bad);
    }
    let mut target = source.clone();
    for bad in rejects {
        assert!(target.decode_state(&bad).is_err());
        assert_eq!(target, source);
    }
}

use super::*;

mod identity;
pub(in crate::emulator) mod layout;
mod legacy;
mod m5_vertical;
mod memptr;
pub(in crate::emulator) mod prefetch;
mod uart_idle;
mod uart_tx;

#[test]
fn future_video_field_identity_is_rejected_atomically_by_bus_and_outer_state() {
    m5_vertical::checks();
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        assert_scroll_key_rejections(region);
        let mut source = machine(region, false);
        while source.timing_snapshot().now().get() < 72 * crate::constants::VDP_LINE_TICKS {
            source.step_instruction().unwrap();
        }
        let saved = source.encode_state().unwrap();
        let (bus_at, video_at) = layout::section_offsets(&saved);
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
            bus.read_state(&mut StateReader::new(
                &malformed[bus_at..bus_at + bus_length]
            ))
            .unwrap_err()
            .to_string()
            .contains("ahead of its calendar")
        );
        assert_eq!(bus, source.bus);
    }
}

fn assert_scroll_key_rejections(region: Region) {
    let mut fetched = machine(region, false);
    fetched.bus.vdp.write_control(0x8144, region).unwrap();
    let at = 2 * region.frame_ticks() + 73 * crate::constants::VDP_LINE_TICKS;
    fetched
        .bus
        .advance_machine(at - fetched.bus.now.get())
        .unwrap();
    let position = fetched.bus.vdp.field_position(region).unwrap();
    assert_eq!(
        fetched.bus.video.horizontal_scroll(position.id, 73),
        Some([0, 0])
    );
    let mut incoming = fetched.clone();
    let at = position.end().unwrap().get() - position.line_ticks;
    incoming
        .bus
        .advance_machine(at - incoming.bus.now.get())
        .unwrap();
    for (case, source) in [(0, fetched.clone()), (1, fetched), (2, incoming)] {
        let saved = source.encode_state().unwrap();
        let mut target = source.clone();
        target.decode_state(&saved).unwrap();
        assert_eq!(target.encode_state().unwrap(), saved);
        let before = target.clone();
        let position = source.bus.vdp.field_position(region).unwrap();
        let (bus_at, video_at) = layout::section_offsets(&saved);
        let mut reader = StateReader::new(&saved);
        reader.set_position(video_at + 15 + crate::constants::FRAMEBUFFER_LEN);
        assert!(reader.read_bool().unwrap());
        reader.set_position(reader.position() + 12);
        reader.read_u8().unwrap();
        assert!(reader.read_bool().unwrap());
        assert_eq!(
            [reader.read_u16().unwrap(), reader.read_u16().unwrap()],
            [0, 0]
        );
        assert!(reader.read_bool().unwrap());
        let key_at = reader.position();
        reader.read_u64().unwrap();
        let old_row = reader.read_u16().unwrap();
        assert!(reader.read_bool().unwrap());
        let (field, row, error) = match case {
            0 => (position.id, position.line + 1, "precedes its fetch"),
            1 => (position.id - 2, old_row, "stale MD scroll fetch field"),
            _ => (position.id + 1, 0, "precedes its fetch"),
        };
        let mut bad = saved.clone();
        bad[key_at..key_at + 8].copy_from_slice(&field.to_le_bytes());
        bad[key_at + 8..key_at + 10].copy_from_slice(&row.to_le_bytes());
        assert!(
            target
                .decode_state(&bad)
                .unwrap_err()
                .to_string()
                .contains(error)
        );
        assert!(target == before);
        let length = u32::from_le_bytes(bad[bus_at - 4..bus_at].try_into().unwrap()) as usize;
        let mut bus = source.bus.clone();
        assert!(
            bus.read_state(&mut StateReader::new(&bad[bus_at..bus_at + length]))
                .unwrap_err()
                .to_string()
                .contains(error)
        );
        assert!(bus == source.bus);
    }
}

pub(in crate::emulator) fn machine(region: Region, concurrent_sound: bool) -> Emulator {
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn complete_machine_state_restores_and_continues() {
    memptr::roundtrip();
    uart_idle::futures();
    uart_tx::futures();
    inserted_save_futures();
    inserted_save_pending_envelopes();
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

fn inserted_save_futures() {
    use crate::cartridge::registers::{LockOn, LockOnSave};
    let mut bytes = crate::cartridge::registers::tests::normal_rom_fixture(LockOn::TwoMiB);
    let mut code = Vec::<u16>::new();
    for value in [0x85, 0x06, 0x90] {
        code.extend([0x13fc, value, 0x00c0, 0x0011]);
    }
    for _ in 0..2 {
        code.extend([0x13fc, 1, 0x00a1, 0x30f1]);
    }
    code.extend([0x41f9, 0x0020, 1, 0x203c, 0x1234, 0x5678]);
    for (offset, opcode) in [(0, 0x0348), (0x200, 0x0548), (0x3f8, 0x0748)] {
        code.extend([0x01c8, offset, opcode, offset]);
    }
    code.extend([
        0x4e70, 0x13fc, 0, 0x00a1, 0x30f1, 0x1039, 0x0020, 1, 0x13c0, 0x00ff, 0, 0x13fc, 1, 0x00a1,
        0x30f1, 0x1039, 0x0020, 0x03ff, 0x13c0, 0x00ff, 1,
    ]);
    let loop_pc = 0x200 + code.len() as u32 * 2;
    code.extend([0x60fe, 0x4e71]);
    for (i, word) in code.into_iter().enumerate() {
        bytes[0x200 + i * 2..0x202 + i * 2].copy_from_slice(&word.to_be_bytes());
    }
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut seed = Emulator::new(
                bytes.clone(),
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            )
            .unwrap();
            seed.bus.cartridge.lock_on = Some(LockOn::TwoMiB);
            seed.bus.cartridge.lock_on_save = Some(LockOnSave::new());
            assert_eq!(
                seed.save_ram_kind(),
                zeff_emu_common::save_ram::SaveRamKind::known_nonvolatile(512)
            );
            for _ in 0..64 {
                if seed.registers().pc == loop_pc {
                    break;
                }
                seed.step_instruction().unwrap();
            }
            assert_eq!(seed.registers().pc, loop_pc);
            for register in [1, 2, 3] {
                assert_eq!(seed.registers().d[register], 0x12345678);
            }
            assert_eq!(seed.reset_pulses(), 1);
            assert!(seed.bus.cartridge.lock_on_save.as_ref().unwrap().enabled);
            assert_eq!(seed.peek(0xff0000), Some(bytes[0x200001]));
            assert_eq!(seed.peek(0xff0001), Some(0x78));
            let mut expected = vec![0xff; 512];
            for start in [0, 256, 508] {
                expected[start..start + 4].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
            }
            assert_eq!(seed.dump_save_ram(), Some(expected.clone()));
            for length in [511, 513] {
                let before = seed.clone();
                assert!(seed.load_save_ram(&vec![0; length]).is_err());
                assert_eq!(seed, before);
            }
            let mut imported = seed.clone();
            imported.load_save_ram(&[0x5a; 512]).unwrap();
            assert_eq!(imported.dump_save_ram(), Some(vec![0x5a; 512]));
            assert_eq!(seed.dump_save_ram(), Some(expected.clone()));

            for enabled in [false, true] {
                for frontend in [false, true] {
                    let mut source = seed.clone();
                    source.bus.cartridge.lock_on_save.as_mut().unwrap().enabled = enabled;
                    assert_eq!(
                        source.save_ram_kind(),
                        zeff_emu_common::save_ram::SaveRamKind::known_nonvolatile(512)
                    );
                    let raw = source.encode_state().unwrap();
                    assert_eq!(&raw[8..12], &STATE_VERSION.to_le_bytes());
                    let front = source.encode_frontend_state().unwrap();
                    let capabilities = layout::capabilities_offset(&raw);
                    assert_eq!(
                        &raw[capabilities..capabilities + 5],
                        &[4, 0, 1, 1, u8::from(enabled)]
                    );
                    for (at, value) in [
                        (capabilities + 3, 2),
                        (capabilities + 4, 2),
                        (capabilities + 5, 1),
                    ] {
                        let mut bad = if frontend { front.clone() } else { raw.clone() };
                        bad[at + if frontend { 16 } else { 0 }] = value;
                        let before = source.clone();
                        if frontend {
                            assert!(source.decode_frontend_state(&bad).is_err());
                        } else {
                            assert!(source.decode_state(&bad).is_err());
                        }
                        assert_eq!(source, before);
                    }
                    let saved = if frontend { front } else { raw.clone() };
                    for trailing in [false, true] {
                        let mut bad = saved.clone();
                        if trailing {
                            bad.push(0);
                        } else {
                            bad.pop();
                        }
                        let before = source.clone();
                        if frontend {
                            assert!(source.decode_frontend_state(&bad).is_err());
                        } else {
                            assert!(source.decode_state(&bad).is_err());
                        }
                        assert_eq!(source, before);
                    }
                    let mut wrong = source.clone();
                    wrong.bus.cartridge.lock_on_save = None;
                    let before = wrong.clone();
                    assert!(wrong.decode_state(&raw).is_err());
                    assert_eq!(wrong, before);
                    let mut wrong = source.clone();
                    wrong.bus.cartridge.lock_on = Some(LockOn::MiB);
                    let before = wrong.clone();
                    assert!(wrong.decode_state(&raw).is_err());
                    assert_eq!(wrong, before);
                    let mut restored = source.clone();
                    restored.load_save_ram(&[0; 512]).unwrap();
                    restored
                        .bus
                        .cartridge
                        .lock_on_save
                        .as_mut()
                        .unwrap()
                        .enabled = !enabled;
                    if frontend {
                        source.decode_frontend_state(&saved).unwrap();
                        restored.decode_frontend_state(&saved).unwrap();
                    } else {
                        source.decode_state(&saved).unwrap();
                        restored.decode_state(&saved).unwrap();
                    }
                    source.drain_audio_samples();
                    restored.drain_audio_samples();
                    for _ in 0..3000 {
                        source.step_instruction().unwrap();
                        restored.step_instruction().unwrap();
                    }
                    assert_eq!(
                        source.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                    assert_eq!(source.framebuffer(), restored.framebuffer());

                    let pcm = source.drain_audio_samples();
                    assert!(pcm.iter().any(|sample| *sample != 0.0));
                    assert_eq!(pcm, restored.drain_audio_samples());
                    assert_eq!(source.dump_save_ram(), Some(expected.clone()));
                    let mut reset = source.clone();
                    reset.reset_cpu().unwrap();
                    assert_eq!(
                        reset.bus.cartridge.lock_on_save,
                        source.bus.cartridge.lock_on_save
                    );
                    reset.bus.cartridge.initial.pc = 0x410000;
                    let before = reset.clone();
                    assert!(reset.reset_cpu().is_err());
                    assert_eq!(reset, before);
                    reset.bus.cartridge.initial = source.bus.cartridge.initial;
                    reset.reset();
                    assert!(!reset.bus.cartridge.lock_on_save.as_ref().unwrap().enabled);
                    assert_eq!(reset.dump_save_ram(), Some(expected.clone()));
                }
            }
        }
    }
    bytes[4..8].copy_from_slice(&0x200000u32.to_be_bytes());
    bytes[0x200000..0x200006].copy_from_slice(&[0x4e, 0x71, 0x60, 0xfe, 0x4e, 0x71]);
    let mut queued = Emulator::new(bytes, LoadConfig::default()).unwrap();
    queued.bus.cartridge.lock_on = Some(LockOn::TwoMiB);
    queued.bus.cartridge.lock_on_save = Some(LockOnSave::new());
    let words = queued.cpu.queued_words();
    queued.bus.cartridge.lock_on_save.as_mut().unwrap().enabled = true;
    let raw = queued.encode_state().unwrap();
    queued.decode_state(&raw).unwrap();
    assert_eq!(queued.cpu.queued_words(), words);
    assert_eq!(queued.peek(0x200000), None);
    assert!(matches!(
        queued.step_instruction(),
        Err(MachineFault::Main(TimingFault::Bus { .. }))
    ));
}

fn inserted_save_pending_envelopes() {
    use crate::cartridge::registers::{LockOn, LockOnSave};
    for enabled in [false, true] {
        for opcode in [false, true] {
            let mut bytes = crate::cartridge::registers::tests::normal_rom_fixture(LockOn::TwoMiB);
            bytes[0x200001] = 0;
            let mut source = Emulator::new(
                bytes,
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound: true,
                },
            )
            .unwrap();
            source.bus.cartridge.lock_on = Some(LockOn::TwoMiB);
            let mut save = LockOnSave::new();
            save.enabled = enabled;
            save.load(&[0; 512]).unwrap();
            source.bus.cartridge.lock_on_save = Some(save);
            source.bus.sound.set_reset(false);
            source.bus.sound.bank = 0x40;
            if opcode {
                source.sound_cpu =
                    zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x8001, 0x1ffe));
            } else {
                source.bus.sound.ram[..4].copy_from_slice(&[0x3a, 1, 0x80, 0x76]);
            }
            for _ in 0..400 {
                ScheduledBus::new(
                    &mut source.bus,
                    &mut source.sound_cpu,
                    &mut source.sound_schedule,
                )
                .advance_machine(1)
                .unwrap();
                assert_eq!(source.sound_fault(), None);
                if source
                    .sound_schedule
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.bank_access.is_some_and(|a| a.address == 0x200001))
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
                0x200001
            );
            let raw = source.encode_state().unwrap();
            let front = source.encode_frontend_state().unwrap();
            let mut reader = StateReader::new(&front);
            reader.set_position(12);
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            let audio = reader
                .read_slice(crate::hardware::audio::MAX_FRONTEND_AUDIO_BYTES)
                .unwrap();
            let mut reader = StateReader::new(&raw);
            reader.set_position(52);
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            let at = reader.position();
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            let end = reader.position();
            for (bank, keep_access) in [(None, false), (None, true), (Some(0), true)] {
                let mut sound = StateWriter::new();
                source
                    .sound_schedule
                    .write_malformed_lock_on_state(&source.sound_cpu, &mut sound, bank, keep_access)
                    .unwrap();
                let mut writer = StateWriter::new();
                writer.write_bytes(&raw[..at]);
                writer.write_vec(&sound.into_bytes());
                writer.write_bytes(&raw[end..]);
                let malformed = writer.into_bytes();
                let mut writer = StateWriter::new();
                writer.write_bytes(b"ZMDFRONT");
                writer.write_u32(1);
                writer.write_vec(&malformed);
                writer.write_vec(audio);
                let envelope = writer.into_bytes();
                for receiving_enabled in [false, true] {
                    for frontend in [false, true] {
                        let mut target = source.clone();
                        target.load_save_ram(&[0x5a; 512]).unwrap();
                        target.bus.cartridge.lock_on_save.as_mut().unwrap().enabled =
                            receiving_enabled;
                        target.bus.sound.bank = 0x1ff;
                        target.bus.ram[0x321] = 0xa6;
                        target.bus.audio.write_psg(target.bus.now.get(), 0x90);
                        let before = target.clone();
                        if frontend {
                            assert!(target.decode_frontend_state(&envelope).is_err());
                        } else {
                            assert!(target.decode_state(&malformed).is_err());
                        }
                        assert_eq!(target, before);
                    }
                }
            }
            source.bus.cartridge.initial.pc = 0x410000;
            let before = source.clone();
            assert!(source.reset_cpu().is_err());
            assert_eq!(source, before);
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn state_identity_bounds_and_late_errors_preserve_live_machine() {
    identity::preserves_live_machine();
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
    layout::bus_prefix(&mut reader);
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

#[test]
fn raw_state_retains_pending_ignored_rom_write_without_cartridge_or_audio_effects() {
    use crate::hardware::sound::schedule::PendingSound;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = machine(region, true);
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 0x1e;
        source.bus.sound.ram[..4].copy_from_slice(&[0x32, 0x6a, 0x80, 0x76]);
        let original_rom = source.bus.cartridge.bytes.clone();
        let original_ram = source.bus.ram.clone();
        source.sound_schedule.pending = Some(
            PendingSound::prepare(
                &source.sound_cpu,
                &source.bus.sound,
                &source.bus.cartridge,
                source.bus.now.get(),
            )
            .unwrap(),
        );
        let pending = source.sound_schedule.pending.as_ref().unwrap();
        assert!(pending.any_bank_access);
        assert_eq!(source.sound_cpu.cycles(), 0);
        let saved = source.encode_state().unwrap();
        let mut restored = machine(region, true);
        restored.decode_state(&saved).unwrap();
        assert_eq!((&restored.encode_state().unwrap()), (&saved));
        assert_eq!(restored.sound_schedule, source.sound_schedule);
        for _ in 0..64 {
            assert_eq!(restored.step_instruction(), source.step_instruction());
        }
        assert!(source.sound_cpu.cycles() >= 13);
        assert_eq!(source.sound_fault(), None);
        assert_eq!(source.bus.cartridge.bytes, original_rom);
        assert_eq!(source.bus.ram, original_ram);
        assert_eq!(
            (&restored.encode_state().unwrap()),
            (&source.encode_state().unwrap())
        );
        assert_eq!(restored.drain_audio_samples(), source.drain_audio_samples());
    }
}

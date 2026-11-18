use super::*;
use crate::save_state::{MAX_STATE_BYTES, STATE_VERSION};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn machine(board: bool) -> Emulator {
    let mut bytes = crate::cartridge::registers::tests::rom(0);
    if !board {
        bytes[0x18e] ^= 1;
    }
    Emulator::new(
        bytes,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap()
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

fn pending(machine: &mut Emulator, read: bool) {
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x80;
    let program: &[u8] = if read {
        &[0x3a, 4, 0x80, 0x76]
    } else {
        &[0x3e, 0x42, 0x32, 4, 0x80, 0x76]
    };
    machine.bus.sound.ram[..program.len()].copy_from_slice(program);
    for _ in 0..400 {
        advance(machine, 1);
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .and_then(|pending| pending.bank_access)
            .is_some_and(|access| access.address == 0x400004)
        {
            return;
        }
    }
    panic!("register phase was not reached");
}

fn latch(machine: &Emulator, address: u32) -> u8 {
    machine.peek(address).unwrap()
}

fn set_latch(machine: &mut Emulator, index: u8, value: u8) {
    machine
        .bus
        .cartridge
        .registers
        .as_mut()
        .unwrap()
        .write(index, value);
}

fn sections(bytes: &[u8]) -> (usize, usize, usize) {
    let mut reader = StateReader::new(bytes);
    reader.set_position(52);
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    let schedule_at = reader.position() + 4;
    let schedule = reader.read_slice(MAX_STATE_BYTES).unwrap();
    let schedule_end = schedule_at + schedule.len();
    reader.read_bool().unwrap();
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    let board_at = reader.position() + 4;
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    assert_eq!(reader.read_slice(MAX_STATE_BYTES).unwrap(), &[0]);
    reader.read_slice(MAX_STATE_BYTES).unwrap();
    assert_eq!(reader.read_slice(MAX_STATE_BYTES).unwrap(), &[0]);
    assert_eq!(reader.read_slice(MAX_STATE_BYTES).unwrap(), &[0]);
    assert_eq!(reader.read_slice(MAX_STATE_BYTES).unwrap(), &[0]);
    assert_eq!(reader.read_slice(MAX_STATE_BYTES).unwrap(), &[0]);
    assert_eq!(
        reader
            .read_slice(crate::save_state::MAX_STATE_BYTES)
            .unwrap(),
        &[0]
    );
    assert!(reader.is_exhausted());
    (schedule_at, schedule_end, board_at)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn pending_register_raw20_frontend1_clone_and_restore_continue_exactly() {
    normal_slot_guest_state_futures();
    malformed_lock_on_envelopes();
    for read in [false, true] {
        let mut source = machine(true);
        set_latch(&mut source, 0, 0xa6);
        set_latch(&mut source, 1, 0x37);
        pending(&mut source, read);
        source.bus.sound.bank = 0x1ff;
        set_latch(&mut source, 0, 0x55);
        let saved = source.encode_state().unwrap();
        assert_eq!(&saved[8..12], &STATE_VERSION.to_le_bytes());
        let front = source.encode_frontend_state().unwrap();
        assert_eq!(&front[8..12], &1_u32.to_le_bytes());
        let mut restored = machine(true);
        set_latch(&mut restored, 0, 0xff);
        set_latch(&mut restored, 1, 0xff);
        restored.decode_frontend_state(&front).unwrap();
        assert_eq!(restored.encode_frontend_state().unwrap(), front);
        let mut rewound = source.clone();
        advance(&mut rewound, 300);
        rewound.decode_state(&saved).unwrap();
        assert_eq!((&rewound.encode_state().unwrap()), (&saved));
        for machine in [&mut source, &mut restored, &mut rewound] {
            if read {
                set_latch(machine, 1, 0xff);
            }
            let deadline = machine.sound_deadline().unwrap().get();
            let now = machine.timing_snapshot().now().get();
            advance(machine, deadline - now - 1);
            assert_eq!(latch(machine, 0x400004), if read { 0xff } else { 0x37 });
            advance(machine, 1);
            assert_eq!(latch(machine, 0x400000), 0x55);
            assert_eq!(latch(machine, 0x400004), if read { 0xff } else { 0x42 });
            if read {
                assert_eq!(machine.sound_cpu.regs().a, 0xff);
            }
            advance(machine, 300);
        }
        assert_eq!(
            source.encode_state().unwrap(),
            restored.encode_state().unwrap()
        );
        assert_eq!(
            (&source.encode_state().unwrap()),
            (&rewound.encode_state().unwrap())
        );
    }
}

fn malformed_lock_on_envelopes() {
    use crate::cartridge::registers::LockOn::{MiB, Patch};
    for active in [false, true] {
        for opcode in [false, true] {
            let mut bytes = crate::cartridge::registers::tests::normal_rom_fixture(MiB);
            if opcode {
                bytes[0x200001] = 0;
                bytes[0x300001] = 0;
            }
            let mut source = Emulator::new(
                bytes,
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound: true,
                },
            )
            .unwrap();
            source.bus.cartridge.lock_on = Some(if active { Patch } else { MiB });
            advance(&mut source, 2000);
            assert!(!source.clone().drain_audio_samples().is_empty());
            source.bus.sound.set_reset(false);
            source.bus.sound.bank = 0x60;
            if opcode {
                source.sound_cpu =
                    zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x8001, 0x1ffe));
            } else {
                source.bus.sound.ram[..4].copy_from_slice(&[0x3a, 1, 0x80, 0x76]);
            }
            for _ in 0..400 {
                advance(&mut source, 1);
                if source
                    .sound_schedule
                    .pending
                    .as_ref()
                    .is_some_and(|pending| {
                        pending
                            .bank_access
                            .is_some_and(|access| access.address == 0x300001)
                    })
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
                0x300001
            );
            let raw = source.encode_state().unwrap();
            let version = STATE_VERSION;
            assert_eq!(&raw[8..12], &version.to_le_bytes());
            let front = source.encode_frontend_state().unwrap();
            let mut reader = StateReader::new(&front);
            reader.set_position(12);
            assert_eq!(reader.read_slice(MAX_STATE_BYTES).unwrap(), raw);
            let audio = reader
                .read_slice(crate::hardware::audio::MAX_FRONTEND_AUDIO_BYTES)
                .unwrap();
            assert!(reader.is_exhausted());
            let mut clean = source.clone();
            clean.decode_state(&raw).unwrap();
            clean.decode_frontend_state(&front).unwrap();
            assert_eq!(clean.encode_frontend_state().unwrap(), front);
            let mut reader = StateReader::new(&raw);
            reader.set_position(52);
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            let schedule_at = reader.position();
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            let schedule_end = reader.position();
            for replay_bank in [None, Some(0)] {
                let mut schedule = StateWriter::new();
                source
                    .sound_schedule
                    .write_malformed_lock_on_state(
                        &source.sound_cpu,
                        &mut schedule,
                        replay_bank,
                        replay_bank.is_some() || !opcode,
                    )
                    .unwrap();
                let mut malformed = StateWriter::new();
                malformed.write_bytes(&raw[..schedule_at]);
                malformed.write_vec(&schedule.into_bytes());
                malformed.write_bytes(&raw[schedule_end..]);
                let malformed = malformed.into_bytes();
                let mut envelope = StateWriter::new();
                envelope.write_bytes(b"ZMDFRONT");
                envelope.write_u32(1);
                envelope.write_vec(&malformed);
                envelope.write_vec(audio);
                let envelope = envelope.into_bytes();
                for receiving_active in [false, true] {
                    for frontend in [false, true] {
                        let mut target = source.clone();
                        target.bus.cartridge.lock_on =
                            Some(if receiving_active { Patch } else { MiB });
                        target.bus.ram[0x321] = 0xa6;
                        target.bus.sound.ram[0x777] = 0x55;
                        target.bus.sound.bank = 0x1ff;
                        let now = target.bus.now.get();
                        target.bus.audio.write_psg(now, 0xa3);
                        let before = target.clone();
                        let error = if frontend {
                            target.decode_frontend_state(&envelope).unwrap_err()
                        } else {
                            target.decode_state(&malformed).unwrap_err()
                        };
                        assert!(error.to_string().contains("lock-on"), "{error:#}");
                        assert_eq!(target, before);
                    }
                }
            }
        }
    }
}

fn normal_slot_guest_state_futures() {
    use crate::cartridge::registers::LockOn::*;
    let addresses = [0x200001_u32, 0x280001, 0x300001, 0x380001, 0x3fffff];
    for mapping in [Empty, HalfMiB, MiB, TwoMiB, Patch] {
        let mut bytes = crate::cartridge::registers::tests::normal_rom_fixture(mapping);
        let mut code: Vec<u16> = Vec::new();
        for value in [0x85, 0x06, 0x90] {
            code.extend([0x13fc, value, 0x00c0, 0x0011]);
        }
        for _ in 0..2 {
            code.extend([0x13fc, u16::from(mapping == Patch), 0x00a1, 0x30f1]);
        }
        code.push(0x4e70);
        for (index, address) in addresses.into_iter().enumerate() {
            code.extend([0x1039, (address >> 16) as u16, address as u16]);
            code.extend([0x13c0, 0x00ff, index as u16]);
        }
        if mapping == Patch {
            code.extend([
                0x13fc, 0, 0x00a1, 0x30f1, 0x1039, 0x003f, 0xffff, 0x13c0, 0x00ff, 5, 0x13fc, 1,
                0x00a1, 0x30f1,
            ]);
        }
        code.push(0x60fe);
        for (index, word) in code.into_iter().enumerate() {
            bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
        }
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent_sound in [false, true] {
                for frontend in [false, true] {
                    let mut source = Emulator::new(
                        bytes.clone(),
                        LoadConfig {
                            region,
                            concurrent_sound,
                        },
                    )
                    .unwrap();
                    let ordinary = source.encode_state().unwrap();
                    source.bus.cartridge.lock_on = Some(mapping);
                    if mapping != Patch {
                        assert_ne!(source.encode_state().unwrap(), ordinary);
                    }
                    source.reset_and_begin_audio_trace(8).unwrap();
                    if mapping == Patch {
                        source
                            .bus
                            .cartridge
                            .lock_on
                            .as_mut()
                            .unwrap()
                            .write_control(1);
                    }
                    for _ in 0..3 {
                        source.step_instruction().unwrap();
                    }
                    let raw = source.encode_state().unwrap();
                    let front = source.encode_frontend_state().unwrap();
                    assert_eq!(&raw[8..12], &STATE_VERSION.to_le_bytes());
                    let mut normal = source.clone();
                    normal
                        .bus
                        .cartridge
                        .lock_on
                        .as_mut()
                        .unwrap()
                        .write_control(0);
                    if mapping == Patch {
                        let capabilities = state::tests::layout::capabilities_offset(&raw);
                        assert_eq!(&raw[capabilities..capabilities + 4], &[3, 1, 1, 1]);
                        let patch_at = capabilities + 2;
                        for (at, value) in [(patch_at, 2), (patch_at + 1, 0), (patch_at + 1, 2)] {
                            let mut bad = raw.clone();
                            bad[at] = value;
                            let mut target = source.clone();
                            let before = target.clone();
                            assert!(target.decode_state(&bad).is_err());
                            assert_eq!(target, before);
                        }
                        for owner in [None, Some(Empty), Some(HalfMiB), Some(TwoMiB)] {
                            let mut target = source.clone();
                            target.bus.cartridge.lock_on = owner;
                            let before = target.clone();
                            assert!(target.decode_state(&raw).is_err());
                            assert_eq!(target, before);
                        }
                    }
                    let mut restored = source.clone();
                    if frontend {
                        source.decode_frontend_state(&front).unwrap();
                        restored.decode_frontend_state(&front).unwrap();
                    } else {
                        source.decode_state(&raw).unwrap();
                        restored.decode_state(&raw).unwrap();
                    }
                    let before = source.clone();
                    assert!(source.load_save_ram(&[0; 512]).is_err());
                    assert_eq!(source, before);
                    let mut malformed = if frontend { front.clone() } else { raw.clone() };
                    malformed.pop();
                    if frontend {
                        assert!(source.decode_frontend_state(&malformed).is_err());
                    } else {
                        assert!(source.decode_state(&malformed).is_err());
                    }
                    assert_eq!(source, before);
                    let expected: Vec<_> = addresses
                        .into_iter()
                        .map(|a| source.peek(a).unwrap())
                        .collect();
                    source.drain_audio_samples();
                    restored.drain_audio_samples();

                    for _ in 0..13 {
                        source.step_instruction().unwrap();
                        restored.step_instruction().unwrap();
                    }
                    for (index, value) in expected.into_iter().enumerate() {
                        assert_eq!(source.peek(0xff0000 + index as u32), Some(value));
                    }
                    assert_eq!(source.bus.cartridge.lock_on, Some(mapping));
                    if mapping == Patch {
                        for _ in 0..4 {
                            source.step_instruction().unwrap();
                            restored.step_instruction().unwrap();
                        }
                        assert_eq!(
                            source.peek(0xff0005),
                            Some(source.bus.cartridge.bytes[0x2fffff])
                        );
                    }
                    for _ in 0..3000 {
                        source.step_instruction().unwrap();
                        restored.step_instruction().unwrap();
                    }
                    assert_eq!(
                        source.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );

                    assert_eq!(source.bus.cartridge.lock_on, Some(mapping));
                    assert_eq!(source.reset_pulses(), 1);

                    assert_eq!(source.framebuffer(), restored.framebuffer());
                    assert!(source.dump_save_ram().is_none());
                    let audio = source.drain_audio_samples();
                    assert!(audio.iter().any(|sample| *sample != 0.0));
                    assert_eq!(audio, restored.drain_audio_samples());
                    assert_eq!(source.finish_audio_trace(), restored.finish_audio_trace());
                    let mut main_reset = source.clone();
                    main_reset.reset_cpu().unwrap();
                    assert_eq!(main_reset.bus.cartridge.lock_on, Some(mapping));
                    if mapping == Patch {
                        main_reset.bus.cartridge.initial.pc = 0x410000;
                        let before = main_reset.clone();
                        assert!(main_reset.reset_cpu().is_err());
                        assert_eq!(main_reset, before);
                    }
                    let mut reset = source.clone();
                    reset.reset();
                    let fresh = if mapping == Patch { MiB } else { mapping };
                    assert_eq!(reset.bus.cartridge.lock_on, Some(fresh));
                    reset.reset_cpu().unwrap();
                    assert_eq!(reset.bus.cartridge.lock_on, Some(fresh));
                    assert_eq!(reset.peek(0x300001), normal.peek(0x300001));
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn corrupt_register_profile_presence_journal_and_truncation_are_transactional() {
    let mut source = machine(true);
    set_latch(&mut source, 0, 0x55);
    pending(&mut source, false);
    let saved = source.encode_state().unwrap();
    let (_, end, board_at) = sections(&saved);
    assert_eq!(&saved[end - 7..end], &[1, 1, 0x42, 0, 0, 0, 0]);
    assert_eq!(&saved[board_at..board_at + 4], &[1, 1, 0x55, 0]);
    let mut corruptions = vec![];
    for (at, value) in [
        (board_at, 0),
        (board_at + 1, 2),
        (end - 6, 2),
        (end - 5, 0x43),
        (end - 4, 2),
    ] {
        let mut bad = saved.clone();
        bad[at] = value;
        corruptions.push(bad);
    }
    corruptions.push(saved[..saved.len() - 1].to_vec());
    for bad in corruptions {
        let mut target = source.clone();
        set_latch(&mut target, 1, 0xa6);
        let before = target.clone();
        assert!(target.decode_state(&bad).is_err());
        assert_eq!(target, before);
    }
    let mut ordinary = machine(false);
    let saved = ordinary.encode_state().unwrap();
    let (_, _, at) = sections(&saved);
    let mut bad = saved;
    bad[at] = 1;
    let before = ordinary.clone();
    assert!(ordinary.decode_state(&bad).is_err());
    assert_eq!(ordinary, before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sound_main_and_machine_reset_preserve_committed_latches_and_drop_pending_writes() {
    let mut source = machine(true);
    set_latch(&mut source, 0, 0xa6);
    set_latch(&mut source, 1, 0x55);
    pending(&mut source, false);
    let mut sound_reset = source.clone();
    sound_reset.reset_sound_cpu();
    assert!(sound_reset.sound_schedule.pending.is_none());
    sound_reset.bus.sound.set_reset(true);
    advance(&mut sound_reset, 300);
    assert_eq!(latch(&sound_reset, 0x400000), 0xa6);
    assert_eq!(latch(&sound_reset, 0x400004), 0x55);
    let mut main_reset = sound_reset.clone();
    main_reset.reset_cpu().unwrap();
    assert_eq!(latch(&main_reset, 0x400000), 0xa6);
    assert_eq!(latch(&main_reset, 0x400004), 0x55);
    source.reset();
    assert!(source.sound_schedule.pending.is_none());
    assert_eq!(latch(&source, 0x400000), 0xa6);
    assert_eq!(latch(&source, 0x400004), 0x55);
    let fresh = machine(true);
    assert_eq!(latch(&fresh, 0x400000), 0);
    assert_eq!(latch(&fresh, 0x400004), 0);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn failed_main_restart_rolls_back_published_cartridge_journal() {
    patch_read_restart_rollback();
    let mut source = machine(true);
    set_latch(&mut source, 0, 0xa6);
    set_latch(&mut source, 1, 0x55);
    pending(&mut source, false);
    let deadline = source.sound_deadline().unwrap().get();
    let now = source.timing_snapshot().now().get();
    advance(&mut source, deadline - now - 1);
    source.bus.cartridge.initial.pc = 0x410000;
    let before = source.clone();
    assert!(source.reset_cpu().is_err());
    assert_eq!(source, before);
    assert_eq!(latch(&source, 0x400000), 0xa6);
    assert_eq!(latch(&source, 0x400004), 0x55);
    advance(&mut source, 1);
    assert_eq!(latch(&source, 0x400004), 0x42);
}

fn patch_read_restart_rollback() {
    use crate::cartridge::registers::LockOn::{MiB, Patch};
    for initial in [MiB, Patch] {
        let mut source = Emulator::new(
            crate::cartridge::registers::tests::normal_rom_fixture(MiB),
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: true,
            },
        )
        .unwrap();
        source.bus.cartridge.lock_on = Some(initial);
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 0x60;
        source.bus.sound.ram[..4].copy_from_slice(&[0x3a, 0, 0x80, 0x76]);
        for _ in 0..400 {
            advance(&mut source, 1);
            if source
                .sound_schedule
                .pending
                .as_ref()
                .and_then(|p| p.bank_access)
                .is_some_and(|a| a.address == 0x300000)
            {
                break;
            }
        }
        assert!(
            source
                .sound_schedule
                .pending
                .as_ref()
                .and_then(|p| p.bank_access)
                .is_some_and(|a| a.address == 0x300000)
        );
        source
            .bus
            .cartridge
            .lock_on
            .as_mut()
            .unwrap()
            .write_control(u8::from(initial == MiB));
        let deadline = source.sound_deadline().unwrap().get();
        let now = source.timing_snapshot().now().get();
        advance(&mut source, deadline - now - 1);
        source.bus.cartridge.initial.pc = 0x410000;
        let before = source.clone();
        assert!(source.reset_cpu().is_err());
        assert_eq!(source, before);
        let expected = source.peek(0x300000).unwrap();
        advance(&mut source, 1);
        assert_eq!(source.sound_cpu().regs().a, expected);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn stopped_public_sound_step_publishes_board_write_and_rejects_mutable_read() {
    for profile in 0..4 {
        let mut bytes = crate::cartridge::registers::tests::rom(profile);
        bytes[0x200..0x208].copy_from_slice(&[0x4e, 0x72, 0x27, 0, 0x4e, 0x71, 0x4e, 0x71]);
        let actual = [0x1c2a_u16, 0x5d98, 0x0211, 0x8ec8][profile];
        let sum = bytes[0x200..]
            .as_chunks::<2>()
            .0
            .iter()
            .fold(0_u16, |sum, pair| {
                sum.wrapping_add(u16::from_be_bytes([pair[0], pair[1]]))
            });
        let tail_at = bytes.len() - 2;
        let tail = u16::from_be_bytes(bytes[tail_at..].try_into().unwrap());
        bytes[tail_at..]
            .copy_from_slice(&tail.wrapping_add(actual).wrapping_sub(sum).to_be_bytes());
        let mut source = Emulator::new(
            bytes,
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: false,
            },
        )
        .unwrap();
        source.step_instruction().unwrap();
        assert_eq!(
            source.step_instruction().unwrap().cpu,
            zeff_m68k::timing::TimedOutcome::Stopped
        );
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 0x80;
        source.bus.sound.ram[..8].copy_from_slice(&[0x3e, 0x42, 0x32, 4, 0x80, 0x3a, 4, 0x80]);
        set_latch(&mut source, 0, 0xa6);
        source.bus.ram[7] = 0xde;
        let first = source.step_sound_instruction().unwrap();
        assert_eq!((first.pc, first.opcode, first.cycles), (0, 0x3e, 7));
        assert_eq!(latch(&source, 0x400004), 0);
        let now = source.timing_snapshot().now().get();
        let main = (source.registers(), source.cpu_clock());
        let write = source.step_sound_instruction().unwrap();
        assert_eq!((write.pc, write.opcode, write.cycles), (2, 0x32, 13));
        assert_eq!(
            source.timing_snapshot().now().get(),
            (now / crate::constants::SOUND_CPU_DIVISOR + 13) * crate::constants::SOUND_CPU_DIVISOR
        );
        assert_eq!((source.registers(), source.cpu_clock()), main);
        assert_eq!(
            (source.sound_cpu().regs().a, source.sound_cpu().regs().pc),
            (0x42, 5)
        );
        assert_eq!(latch(&source, 0x400000), 0xa6);
        assert_eq!(latch(&source, 0x400004), 0x42);
        assert_eq!(source.bus.ram[7], 0xde);
        let before = source.clone();
        assert_eq!(
            source.step_sound_instruction(),
            Err(SoundFault::UnsupportedAccess {
                address: 0x8004,
                direction: zeff_m68k::bus::Direction::Read
            })
        );
        assert_eq!(source, before);
    }
}

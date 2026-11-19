use super::*;

fn pending(source: &mut Emulator, target: u32, write: bool) {
    source.bus.sound.set_reset(false);
    source.bus.sound.bank = (target >> 15) as u16;
    let address = (0x8000 | (target & 0x7fff)) as u16;
    let [low, high] = address.to_le_bytes();
    let program = if write {
        vec![0x3e, 0x42, 0x32, low, high, 0x76]
    } else {
        vec![0x3a, low, high, 0x76]
    };
    source.bus.sound.ram[..program.len()].copy_from_slice(&program);
    for _ in 0..600 {
        advance(source, 1);
        if source
            .sound_schedule
            .pending
            .as_ref()
            .and_then(|pending| pending.bank_access)
            .is_some_and(|access| {
                access.address == target
                    && access.direction
                        == if write {
                            zeff_m68k::bus::Direction::Write
                        } else {
                            zeff_m68k::bus::Direction::Read
                        }
            })
        {
            return;
        }
    }
    panic!("SF transfer was not reached");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_whole_machine_pending_raw_and_frontend_restore_preserve_publication() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let target = if revision == Revision::HiddenSave {
            0x3c0012
        } else {
            0x400012
        };
        for write in [false, true] {
            for phase in 0..3 {
                let mut source = machine(revision, &[0x60fe], Region::NtscUs);
                source
                    .bus
                    .cartridge
                    .sf001
                    .as_mut()
                    .unwrap()
                    .control(0xe00, 0x80);
                source
                    .bus
                    .cartridge
                    .sf001
                    .as_mut()
                    .unwrap()
                    .write(target, 0x17);
                pending(&mut source, target, write);
                let deadline = source.sound_deadline().unwrap().get();
                let remaining = deadline - source.bus.now.get();
                advance(
                    &mut source,
                    match phase {
                        0 => 0,
                        1 => 1,
                        _ => remaining - 1,
                    },
                );
                let raw = source.encode_state().unwrap();
                let front = source.encode_frontend_state().unwrap();
                let mut restored = machine(revision, &[0x60fe], Region::NtscUs);
                restored.decode_state(&raw).unwrap();
                let mut frontend = machine(revision, &[0x60fe], Region::NtscUs);
                frontend.decode_frontend_state(&front).unwrap();
                for copy in [&mut source, &mut restored, &mut frontend] {
                    assert_eq!(copy.peek(target), Some(0x17));
                    let remaining = deadline - copy.bus.now.get();
                    advance(copy, remaining - 1);
                    assert_eq!(copy.peek(target), Some(0x17));
                    advance(copy, 1);
                    assert_eq!(copy.peek(target), Some(if write { 0x42 } else { 0x17 }));
                    if !write {
                        assert_eq!(copy.sound_cpu.regs().a, 0x17);
                    }
                    advance(copy, 200);
                }
                assert_eq!(
                    (&source.encode_state().unwrap()),
                    (&restored.encode_state().unwrap())
                );
                assert_eq!(
                    source.encode_state().unwrap(),
                    frontend.encode_state().unwrap()
                );
                assert_eq!(source.framebuffer(), restored.framebuffer());
                assert_eq!(source.drain_audio_samples(), frontend.drain_audio_samples());
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_scheduled_main_read_and_write_observe_prior_sound_transfer() {
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::ClockedBus;
    let access = |address| BusAccess {
        logical_address: address,
        bus_address: address,
        lanes: ByteLanes::Both,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    };
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let target = if revision == Revision::HiddenSave {
            0x3c0012
        } else {
            0x400012
        };
        for write in [false, true] {
            let mut source = machine(revision, &[0x60fe], Region::NtscUs);
            source
                .bus
                .cartridge
                .sf001
                .as_mut()
                .unwrap()
                .control(0xe00, 0x80);
            source
                .bus
                .cartridge
                .sf001
                .as_mut()
                .unwrap()
                .write(target, 0x17);
            pending(&mut source, target, write);
            let start = source.bus.cpu_clock;
            let mut bus = ScheduledBus::new(
                &mut source.bus,
                &mut source.sound_cpu,
                &mut source.sound_schedule,
            );
            if write {
                let result = bus.read_cycle(access(target), start);
                assert!(result.wait_clocks > 0);
                assert_eq!(result.result, Ok(0x42ff));
            } else {
                let result = bus.write_cycle(access(0xe00), 0x0040, start);
                assert!(result.wait_clocks > 0);
                assert_eq!(result.result, Ok(()));
                assert_eq!(bus.cpu.regs().a, 0x17);
                assert_eq!(
                    bus.bus.cartridge.sf001.as_ref().unwrap().mapping(),
                    Mapping::Disconnected
                );
            }
            assert_eq!(source.sound_fault(), None);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_raw_and_frontend_states_continue_every_mapping_lock_revision() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        for mode in [0, 0x80, 0x40] {
            for lock in [0, 0x20] {
                let words = [0x60fe];
                let mut source = machine(revision, &words, Region::NtscUs);
                let cells: Vec<_> = (0..0x10000)
                    .map(|index| (index * 37 + (index >> 8)) as u8)
                    .collect();
                source.load_save_ram(&cells).unwrap();
                source
                    .bus
                    .cartridge
                    .sf001
                    .as_mut()
                    .unwrap()
                    .control(0xe00, mode | lock);
                let raw = source.encode_state().unwrap();
                let frontend = source.encode_frontend_state().unwrap();
                let mut restored = machine(revision, &words, Region::NtscUs);
                restored.decode_state(&raw).unwrap();
                assert_eq!(
                    (&source.encode_state().unwrap()),
                    (&restored.encode_state().unwrap())
                );
                let mut front = machine(revision, &words, Region::NtscUs);
                front.decode_frontend_state(&frontend).unwrap();
                assert_eq!(
                    source.encode_state().unwrap(),
                    front.encode_state().unwrap()
                );
                for _ in 0..4 {
                    let expected = source.step_instruction();
                    assert_eq!(restored.step_instruction(), expected);
                    assert_eq!(front.step_instruction(), expected);
                    if expected.is_err() {
                        assert_eq!(restored.cpu, source.cpu);
                        assert_eq!(front.cpu, source.cpu);
                        assert_eq!(restored.bus.now, source.bus.now);
                        assert_eq!(front.bus.now, source.bus.now);
                        assert_eq!(restored.bus.cartridge, source.bus.cartridge);
                        assert_eq!(front.bus.cartridge, source.bus.cartridge);
                        break;
                    }
                    assert_eq!(
                        (&restored.encode_state().unwrap()),
                        (&source.encode_state().unwrap())
                    );
                    assert_eq!(
                        front.encode_state().unwrap(),
                        source.encode_state().unwrap()
                    );
                    assert_eq!(source.dump_save_ram(), Some(cells.clone()));
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_legacy_wrong_rom_bad_board_tags_lengths_and_trailing_are_transactional() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut source = machine(revision, &[0x60fe], Region::NtscUs);
        source
            .bus
            .cartridge
            .sf001
            .as_mut()
            .unwrap()
            .control(0xe00, 0xa0);
        let saved = source.encode_state().unwrap();
        let board_at = crate::emulator::state::tests::layout::sf001_offset(&saved) + 1;
        assert_eq!(saved[board_at - 1], 1);
        assert_eq!(
            &saved[board_at + 3..board_at + 7],
            &0x10000_u32.to_le_bytes()
        );
        let mut bad_states = Vec::new();
        for version in [0_u32, crate::save_state::STATE_VERSION + 1] {
            let mut bad = saved.clone();
            bad[8..12].copy_from_slice(&version.to_le_bytes());
            bad_states.push(bad);
        }
        for (at, value) in [
            (board_at, 3),
            (board_at + 1, 3),
            (board_at + 2, 2),
            (board_at + 3, 1),
            (12, 0xff),
        ] {
            let mut bad = saved.clone();
            bad[at] = value;
            bad_states.push(bad);
        }
        let mut extra = saved.clone();
        extra.push(0);
        bad_states.push(extra);
        for end in [0, 8, 12, 48, board_at, saved.len() - 1] {
            bad_states.push(saved[..end].to_vec());
        }
        for bad in bad_states {
            let before = source.clone();
            assert!(source.decode_state(&bad).is_err());
            assert_eq!(source, before);
        }
        let mut wrong = machine(
            if revision == Revision::HiddenSave {
                Revision::UpperSave
            } else {
                Revision::HiddenSave
            },
            &[0x60fe],
            Region::NtscUs,
        );
        let before = wrong.clone();
        assert!(wrong.decode_state(&saved).is_err());
        assert_eq!(wrong, before);
    }
    let mut ordinary = crate::cartridge::sf001::tests::rom(Revision::HiddenSave);
    ordinary[0x18e] ^= 1;
    let mut ordinary = Emulator::new(ordinary, LoadConfig::default()).unwrap();
    let mut saved = ordinary.encode_state().unwrap();
    assert_eq!(
        &saved[8..12],
        &crate::save_state::STATE_VERSION.to_le_bytes()
    );
    let presence = crate::emulator::state::tests::layout::sf001_offset(&saved);
    assert_eq!(saved[presence], 0);
    saved[presence] = 1;
    let before = ordinary.clone();
    assert!(ordinary.decode_state(&saved).is_err());
    assert_eq!(ordinary, before);
}

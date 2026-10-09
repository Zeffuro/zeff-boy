use super::*;

fn legacy_pair_digest(machines: [&Emulator; 2], schedule: &Schedule) -> [u8; 32] {
    let mut pair = Sha256::new();
    pair.update(b"zeff-ws-pair-checkpoint-v1\0");
    for machine in machines {
        let mut hash = Sha256::new();
        hash.update(b"zeff-ws-pair-machine-v1\0");
        hash.update(machine.encode_state().unwrap());
        machine.cpu.hash_rollback_runtime(&mut hash);
        machine.bus.apu.hash_rollback_runtime(&mut hash);
        machine.bus.hash_rollback_service(&mut hash);
        pair.update(hash.finalize());
    }
    for epoch in schedule.epochs {
        pair.update(epoch.to_le_bytes());
    }
    pair.update(schedule.frame.to_le_bytes());
    pair.update((schedule.events.len() as u64).to_le_bytes());
    for event in &schedule.events {
        pair.update(event.tick.to_le_bytes());
        pair.update([event.sender as u8, event.byte]);
        pair.update(event.generation.to_le_bytes());
        pair.update(event.baud_bps.to_le_bytes());
    }
    pair.finalize().into()
}

fn event(tick: u64, sender: usize, generation: u64) -> CableEvent {
    CableEvent {
        tick,
        sender,
        generation,
        byte: 0x41 + sender as u8,
        baud_bps: if sender == 0 { 9600 } else { 38400 },
    }
}

#[test]
fn checkpoints_match_legacy_digest_and_preserve_snapshot_ownership_and_accounting() {
    for color in [false, true] {
        for runtime in [false, true] {
            let mut left = machine(color, &[0x90, 0xf4]);
            let mut right = left.clone_for_link_peer();
            if runtime {
                left.step_instruction();
                right.step_instruction();
                left.bus.apu.step_cycles(64, &left.bus.ram);
                right.bus.apu.step_cycles(64, &right.bus.ram);
                assert!(left.apu_debug_snapshot().buffered_samples > 0);
                left.bus.begin_frame_service();
                left.bus.step_cycles(1);
                assert!(left.bus.frame_service_state_for_test().0);
            }
            let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
            let empty = pair.capture([&left, &right]).unwrap();
            assert_eq!(
                empty.checksum(),
                legacy_pair_digest([&left, &right], &pair.schedule)
            );
            pair.schedule.frame = 2;
            pair.schedule.events = vec![event(33, 0, 3), event(33, 0, 4), event(33, 1, 3)];
            let saved = pair.capture([&left, &right]).unwrap();
            assert_ne!(saved.checksum(), empty.checksum());
            assert_eq!(
                saved.checksum(),
                legacy_pair_digest([&left, &right], &pair.schedule)
            );
            let retained = std::mem::size_of::<WonderSwanPairSnapshot>()
                + left.encode_state().unwrap().len()
                + right.encode_state().unwrap().len()
                + left.bus.apu.rollback_runtime_bytes()
                + right.bus.apu.rollback_runtime_bytes()
                + left.cartridge_rom_bytes().len()
                + pair.schedule.events.capacity() * std::mem::size_of::<CableEvent>();
            assert_eq!(saved.retained_bytes(), retained);
            assert_eq!(saved.shared_media_bytes(), left.cartridge_rom_bytes().len());
            pair.advance_frame([&mut left, &mut right], [0x701, 0x152])
                .unwrap();
            pair.restore([&mut left, &mut right], &saved).unwrap();
            assert_eq!(
                pair.capture([&left, &right]).unwrap().checksum(),
                saved.checksum()
            );
            assert_eq!(
                saved.checksum(),
                legacy_pair_digest([&left, &right], &pair.schedule)
            );
            assert_eq!(saved.retained_bytes(), retained);
            assert!(saved.owner.ptr_eq(&Arc::downgrade(&pair.owner)));
            for (live, snapshot) in [&left, &right].into_iter().zip(&saved.machines) {
                assert!(Arc::ptr_eq(&live.link_identity, &snapshot.link_identity));
                assert!(std::ptr::eq(
                    live.cartridge_rom_bytes().as_ptr(),
                    snapshot.cartridge_rom_bytes().as_ptr()
                ));
            }
        }
    }
}

fn assert_rejected_without_mutation(
    pair: &mut WonderSwanLinkPair,
    left: &mut Emulator,
    right: &mut Emulator,
    snapshot: &WonderSwanPairSnapshot,
    error: &str,
) {
    let before = pair.capture([&*left, &*right]).unwrap();
    let identities = [left.link_identity.clone(), right.link_identity.clone()];
    let media = [
        left.bus.cartridge.shared_rom(),
        right.bus.cartridge.shared_rom(),
    ];
    assert_eq!(
        pair.restore([&mut *left, &mut *right], snapshot)
            .unwrap_err()
            .to_string(),
        error
    );
    let after = pair.capture([&*left, &*right]).unwrap();
    assert_eq!(after.checksum(), before.checksum());
    assert_eq!(after.frame(), before.frame());
    assert_eq!(after.retained_bytes(), before.retained_bytes());
    assert_eq!(after.shared_media_bytes(), before.shared_media_bytes());
    assert_eq!(
        pair.expected,
        state::machine_hashes([&*left, &*right]).unwrap()
    );
    for (i, machine) in [left, right].into_iter().enumerate() {
        assert!(Arc::ptr_eq(&machine.link_identity, &identities[i]));
        assert!(Arc::ptr_eq(&machine.bus.cartridge.shared_rom(), &media[i]));
    }
}

#[test]
fn invalid_schedule_and_runtime_corruption_restore_fail_before_mutation() {
    let mut left = machine(false, &[]);
    let mut right = machine(false, &[]);
    left.bus.apu.step_cycles(64, &left.bus.ram);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    let saved = pair.capture([&left, &right]).unwrap();
    pair.advance_frame([&mut left, &mut right], [0x701, 0x152])
        .unwrap();
    for (events, error) in [
        (
            vec![event(1, 0, 1); MAX_CABLE_EVENTS + 1],
            "paired snapshot cable queue overflow",
        ),
        (vec![event(1, 2, 1)], "invalid paired cable event"),
        (
            vec![CableEvent {
                baud_bps: 19200,
                ..event(1, 0, 1)
            }],
            "invalid paired cable event",
        ),
        (
            vec![event(2, 0, 1), event(1, 0, 1)],
            "unordered paired cable events",
        ),
        (
            vec![event(1, 1, 1), event(1, 0, 1)],
            "unordered paired cable events",
        ),
        (
            vec![event(1, 0, 2), event(1, 0, 1)],
            "unordered paired cable events",
        ),
    ] {
        let mut corrupt = saved.clone();
        corrupt.schedule.events = events;
        corrupt.machines[0].debug.break_on_next = true;
        assert_rejected_without_mutation(&mut pair, &mut left, &mut right, &corrupt, error);
    }
    let mut corrupt = saved.clone();
    corrupt.machines[0].drain_audio_samples_into(&mut Vec::new());
    assert_eq!(
        corrupt.machines[0].encode_state().unwrap(),
        saved.machines[0].encode_state().unwrap()
    );
    assert_rejected_without_mutation(
        &mut pair,
        &mut left,
        &mut right,
        &corrupt,
        "paired snapshot checkpoint differs",
    );
    let mut corrupt = saved.clone();
    corrupt.machines[0].debug.break_on_next = true;
    assert_rejected_without_mutation(
        &mut pair,
        &mut left,
        &mut right,
        &corrupt,
        "paired link excludes debugger and traces",
    );
}

use super::*;

fn configure_audio(machine: &mut Emulator) {
    machine.bus.ram[..16].fill(0xf0);
    for (port, value) in [(0x80, 0), (0x81, 7), (0x88, 0xff), (0x90, 1)] {
        machine.io_write8(port, value);
    }
}

#[test]
fn pending_tx_snapshot_replays_exact_states_pcm_and_rgb() {
    for color in [false, true] {
        let mut left = machine(color, &[]);
        let mut right = machine(color, &[]);
        configure_audio(&mut left);
        configure_audio(&mut right);
        uart(&mut left, true, Some(0x4e));
        uart(&mut right, false, Some(0x9b));
        let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
        pair.advance_to([&mut left, &mut right], 399).unwrap();
        assert_eq!(left.uart_debug_snapshot().tx_cycles_remaining, 401);
        assert!(left.apu_debug_snapshot().buffered_samples > 0);
        let saved = pair.capture([&left, &right]).unwrap();
        let first = pair
            .advance_frame([&mut left, &mut right], [0x701, 0x152])
            .unwrap();
        let final_state = pair.capture([&left, &right]).unwrap();
        let rgb = pair.video_checksum([&left, &right]).unwrap();
        pair.restore([&mut left, &mut right], &saved).unwrap();
        assert_eq!(
            pair.capture([&left, &right]).unwrap().checksum(),
            saved.checksum()
        );
        let replay = pair
            .advance_frame([&mut left, &mut right], [0x701, 0x152])
            .unwrap();
        assert_eq!(first.audio, replay.audio);
        assert!(
            first
                .audio
                .iter()
                .flatten()
                .any(|sample| sample.abs() > 0.001)
        );
        assert_eq!(
            pair.capture([&left, &right]).unwrap().checksum(),
            final_state.checksum()
        );
        assert_eq!(pair.video_checksum([&left, &right]).unwrap(), rgb);
        assert_eq!(
            left.encode_state().unwrap(),
            final_state.machines[0].encode_state().unwrap()
        );
        assert_eq!(
            right.encode_state().unwrap(),
            final_state.machines[1].encode_state().unwrap()
        );
        assert!(saved.retained_bytes() > left.cartridge_rom_bytes().len() * 2);
    }
}

#[test]
fn pending_cable_rx_and_deferred_service_survive_atomic_restore() {
    let mut left = machine(false, &[0x90; 16]);
    let mut right = machine(false, &[0xe4, 0xb1, 0xf4]);
    uart(&mut left, true, Some(0x73));
    uart(&mut right, true, None);
    let mut pending = left.bus.uart_save_state();
    pending.tx_cycles_remaining = 5;
    left.bus.load_uart_save_state(pending);
    left.bus.begin_frame_service();
    left.bus.step_cycles(1);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    let deferred = pair.capture([&left, &right]).unwrap();
    pair.advance_to([&mut left, &mut right], 3).unwrap();
    // The receiver crossed the completion while the sender still precedes it.
    assert_eq!(pair.pending_events(), 0);
    pair.restore([&mut left, &mut right], &deferred).unwrap();
    assert_eq!(
        pair.capture([&left, &right]).unwrap().checksum(),
        deferred.checksum()
    );
    pair.schedule.events.push(CableEvent {
        tick: 10,
        sender: 0,
        generation: 1,
        byte: 0x42,
        baud_bps: 38400,
    });
    let cable = pair.capture([&left, &right]).unwrap();
    pair.advance_to([&mut left, &mut right], 10).unwrap();
    let delivered = pair.capture([&left, &right]).unwrap();
    assert_eq!(right.uart_debug_snapshot().rx_data, 0x73);
    assert_eq!(right.uart_debug_snapshot().status & 3, 3);
    pair.restore([&mut left, &mut right], &cable).unwrap();
    assert_eq!(pair.pending_events(), 1);
    pair.advance_to([&mut left, &mut right], 10).unwrap();
    assert_eq!(
        pair.capture([&left, &right]).unwrap().checksum(),
        delivered.checksum()
    );
}

#[test]
fn extracted_tx_waiting_for_receiver_replays_exact_delivery() {
    let mut left = machine(false, &[0xe4, 0xb1, 0xf4]);
    let mut right = machine(false, &[0x90; 16]);
    uart(&mut left, true, Some(0x52));
    uart(&mut right, true, None);
    let mut pending = left.bus.uart_save_state();
    pending.tx_cycles_remaining = 5;
    left.bus.load_uart_save_state(pending);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    pair.run(&mut [&mut left, &mut right], [7, 3]).unwrap();
    pair.expected = state::machine_hashes([&left, &right]).unwrap();
    assert_eq!(pair.pending_events(), 1);
    assert_eq!(pair.schedule.events[0].tick, 5);
    assert_eq!(right.uart_debug_snapshot().status & 1, 0);
    let saved = pair.capture([&left, &right]).unwrap();
    pair.advance_to([&mut left, &mut right], 8).unwrap();
    assert_eq!(right.uart_debug_snapshot().rx_data, 0x52);
    let delivered = pair.capture([&left, &right]).unwrap();
    pair.restore([&mut left, &mut right], &saved).unwrap();
    assert_eq!(pair.pending_events(), 1);
    pair.advance_to([&mut left, &mut right], 8).unwrap();
    assert_eq!(
        pair.capture([&left, &right]).unwrap().checksum(),
        delivered.checksum()
    );
}

#[test]
fn foreign_machine_owner_and_corrupt_snapshot_fail_before_mutation() {
    let mut left = machine(false, &[]);
    let mut right = machine(false, &[]);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    let saved = pair.capture([&left, &right]).unwrap();
    let foreign = machine(false, &[]);
    assert!(pair.capture([&foreign, &right]).is_err());
    let mut other = WonderSwanLinkPair::new([&left, &right]).unwrap();
    assert!(other.restore([&mut left, &mut right], &saved).is_err());
    let before = state::machine_hashes([&left, &right]).unwrap();
    let mut corrupt = saved.clone();
    corrupt.machines[1].bus.ram[0] ^= 1;
    assert!(pair.restore([&mut left, &mut right], &corrupt).is_err());
    assert_eq!(state::machine_hashes([&left, &right]).unwrap(), before);
    let mut corrupt = saved.clone();
    corrupt.schedule.epochs[0] += 1;
    assert!(pair.restore([&mut left, &mut right], &corrupt).is_err());
    assert_eq!(state::machine_hashes([&left, &right]).unwrap(), before);
    left.io_write8(0xb3, 0xc0);
    assert!(pair.capture([&left, &right]).is_err());
    assert!(pair.advance_frame([&mut left, &mut right], [0; 2]).is_err());
    assert!(pair.restore([&mut left, &mut right], &saved).is_err());
}

#[test]
fn scheduler_fault_can_restore_owned_checkpoint_but_external_mutation_cannot() {
    let mut left = machine(false, &[0x90, 0xc6, 0xc8, 0]);
    let mut right = machine(false, &[]);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    let saved = pair.capture([&left, &right]).unwrap();
    assert!(pair.advance_frame([&mut left, &mut right], [0; 2]).is_err());
    assert!(left.last_trap().is_some());
    assert!(pair.capture([&left, &right]).is_err());
    left.debug.break_on_next = true;
    assert!(pair.restore([&mut left, &mut right], &saved).is_err());
    left.debug.break_on_next = false;
    pair.restore([&mut left, &mut right], &saved).unwrap();
    assert_eq!(
        pair.capture([&left, &right]).unwrap().checksum(),
        saved.checksum()
    );
    pair.advance_to([&mut left, &mut right], 1).unwrap();
    assert!(left.last_trap().is_none());
    assert!(pair.advance_frame([&mut left, &mut right], [0; 2]).is_err());
    left.bus.ram[0x300] ^= 1;
    let before = state::machine_hashes([&left, &right]).unwrap();
    assert!(pair.restore([&mut left, &mut right], &saved).is_err());
    assert_eq!(state::machine_hashes([&left, &right]).unwrap(), before);
}

#[test]
fn runtime_omissions_affect_checkpoint_without_changing_native_format() {
    let mut left = machine(false, &[]);
    let right = machine(false, &[]);
    left.bus.apu.step_cycles(64, &left.bus.ram);
    let pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    let saved = pair.capture([&left, &right]).unwrap();
    let native = left.encode_state().unwrap();
    let mut corrupt = saved.clone();
    let mut samples = Vec::new();
    corrupt.machines[0].drain_audio_samples_into(&mut samples);
    assert!(!samples.is_empty());
    assert_eq!(corrupt.machines[0].encode_state().unwrap(), native);
    assert_ne!(
        state::pair_hash(
            [&corrupt.machines[0], &corrupt.machines[1]],
            &corrupt.schedule
        )
        .unwrap(),
        saved.checksum()
    );
    left.drain_audio_samples_into(&mut Vec::new());
    assert!(pair.capture([&left, &right]).is_err());
    assert_eq!(crate::save_state::SAVE_STATE_FORMAT_VERSION, 14);
}

#[test]
fn bounds_and_trace_contracts_reject_invalid_entry() {
    let mut left = machine(false, &[]);
    let mut right = machine(false, &[]);
    let mut pair = WonderSwanLinkPair::new([&left, &right]).unwrap();
    let before = pair.capture([&left, &right]).unwrap().checksum();
    assert!(
        pair.advance_frame([&mut left, &mut right], [0x800, 0])
            .is_err()
    );
    assert!(
        pair.advance_to([&mut left, &mut right], u64::from(CYCLES_PER_FRAME) * 2 + 1)
            .is_err()
    );
    assert_eq!(pair.capture([&left, &right]).unwrap().checksum(), before);
    left.set_apu_channel_mutes([true, false, false, false]);
    assert!(WonderSwanLinkPair::new([&left, &right]).is_err());
    left.set_apu_channel_mutes([false; 4]);
    left.debug.break_on_next = true;
    assert!(WonderSwanLinkPair::new([&left, &right]).is_err());
}

#[test]
fn immutable_media_is_shared_while_mutable_cartridge_state_is_isolated() {
    let mut rom = machine(false, &[]).cartridge_rom_bytes().to_vec();
    let footer = rom.len() - 10;
    rom[footer + 5] = 2;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    let source = Emulator::new(&rom, 48_000).unwrap();
    let mut peer = source.clone_for_link_peer();
    assert!(std::ptr::eq(
        source.cartridge_rom_bytes().as_ptr(),
        peer.cartridge_rom_bytes().as_ptr()
    ));
    peer.bus.cartridge.rom_write8(0x10000, 0x35);
    peer.bus.cartridge.set_bank0(3);
    assert_eq!(source.dump_battery_sram().unwrap()[0], 0xff);
    assert_eq!(peer.dump_battery_sram().unwrap()[0], 0x35);
    assert_ne!(source.bus.cartridge.bank0(), peer.bus.cartridge.bank0());
    let mut pair = WonderSwanLinkPair::new([&source, &peer]).unwrap();
    let saved = pair.capture([&source, &peer]).unwrap();
    assert_eq!(saved.shared_media_bytes(), rom.len());
    assert!(std::ptr::eq(
        source.cartridge_rom_bytes().as_ptr(),
        saved.machines[0].cartridge_rom_bytes().as_ptr()
    ));
    assert_eq!(
        saved.native_state(Endpoint::Zero).unwrap(),
        source.encode_state().unwrap()
    );
    let mut corrupt = saved.clone();
    rom[0] ^= 1;
    corrupt.machines[0].bus.cartridge = crate::hardware::cartridge::Cartridge::load(&rom).unwrap();
    assert_eq!(corrupt.machines[0].rom_hash(), source.rom_hash());
    let mut local = source.clone();
    let before = state::machine_hashes([&local, &peer]).unwrap();
    assert!(pair.restore([&mut local, &mut peer], &corrupt).is_err());
    assert_eq!(state::machine_hashes([&local, &peer]).unwrap(), before);
    local.bus.cartridge = crate::hardware::cartridge::Cartridge::load(&rom).unwrap();
    assert!(pair.capture([&local, &peer]).is_err());
    assert!(WonderSwanLinkPair::new([&local, &peer]).is_err());
}

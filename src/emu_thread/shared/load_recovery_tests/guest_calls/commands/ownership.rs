use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_common_undo_ownership_configuration_and_lifecycle_are_atomic() {
    let mut owner = Owner::new(standard());
    owner.refused_undo(Vec::new());
    let first = owner.call();
    let mut other = Owner::new(standard());
    let foreign = other.call();
    assert_ne!(first, foreign);
    for token in [Vec::new(), vec![0; 15], vec![0; 16], foreign] {
        owner.refused_undo(token);
    }
    let before_second = frontend(&owner.backend);
    let second_host = session(&mut owner.backend);
    let second = owner.call();
    assert_ne!(first, second);
    owner.refused_undo(first);
    let called = frontend(&owner.backend);
    let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(0x1200, 4)));
    assert!(!effects.potentially_dirty);
    assert!(matches!(
        effects.response,
        Some(EmuResponse::GuestCallFailed { .. })
    ));
    assert_eq!(frontend(&owner.backend), called);
    owner.undo(second.clone());
    assert_eq!(frontend(&owner.backend), before_second);
    assert_eq!(session(&mut owner.backend), second_host);
    owner.refused_undo(second);

    for change in 0..4 {
        let mut owner = Owner::new(standard());
        let original = frontend(&owner.backend);
        let host = session(&mut owner.backend);
        let token = owner.call();
        match change {
            0 => owner.backend.set_sample_rate(44_100),
            1 => owner.backend.set_apu_sample_generation_enabled(false),
            2 => owner.backend.set_apu_channel_mutes(&[true; 10]),
            _ => {
                let EmuBackend::Md(md) = &mut owner.backend else {
                    unreachable!()
                };
                md.set_controllers(ControllerKind::SixButton);
            }
        }
        owner.refused_undo(token.clone());
        match change {
            0 => owner.backend.set_sample_rate(48_000),
            1 => owner.backend.set_apu_sample_generation_enabled(true),
            2 => owner.backend.set_apu_channel_mutes(&[false; 10]),
            _ => {
                let EmuBackend::Md(md) = &mut owner.backend else {
                    unreachable!()
                };
                md.set_controllers(ControllerKind::ThreeButton);
            }
        }
        owner.undo(token);
        assert_eq!(frontend(&owner.backend), original, "change {change}");
        assert_eq!(session(&mut owner.backend), host, "change {change}");
    }

    let mut owner = Owner::new(standard());
    let original = frontend(&owner.backend);
    let token = owner.call();
    let overlay = crate::cheats::parse_cheat_for_system("001800:1234", ActiveSystem::Md)
        .unwrap()
        .0;
    machine_mut(&mut owner.backend)
        .install_rom_patches(&overlay)
        .unwrap();
    assert_eq!(machine(&owner.backend).peek(0x1800), Some(0x12));
    owner.undo(token);
    assert_eq!(frontend(&owner.backend), original);
    assert_eq!(machine(&owner.backend).peek(0x1800), Some(0x12));

    let mut owner = Owner::new(standard());
    let original = frontend(&owner.backend);
    let token = owner.call();
    let called = frontend(&owner.backend);
    let host = session(&mut owner.backend);
    assert!(owner.backend.load_state_from_bytes(vec![0]).is_err());
    assert!(owner.backend.restore_rewind_checkpoint(vec![0]).is_err());
    assert_eq!(frontend(&owner.backend), called);
    assert_eq!(session(&mut owner.backend), host);
    owner.undo(token);
    assert_eq!(frontend(&owner.backend), original);

    for operation in 0..4 {
        let mut owner = Owner::new(if operation == 3 {
            guest_call_backend()
        } else {
            standard()
        });
        let raw = owner.backend.encode_state_bytes().unwrap();
        let checkpoint = frontend(&owner.backend);
        let token = owner.call();
        let called = frontend(&owner.backend);
        let host = session(&mut owner.backend);
        assert!(owner.backend.load_state_from_bytes(vec![0]).is_err());
        assert!(owner.backend.restore_rewind_checkpoint(vec![0]).is_err());
        assert_eq!(frontend(&owner.backend), called);
        assert_eq!(session(&mut owner.backend), host);
        match operation {
            0 => {
                owner.backend.load_state_from_bytes(raw).unwrap();
            }
            1 => {
                owner.backend.restore_rewind_checkpoint(checkpoint).unwrap();
            }
            2 => owner.backend.reset(),
            _ => {
                let EmuBackend::Md(md) = &mut owner.backend else {
                    unreachable!()
                };
                md.try_warm_reset().unwrap();
            }
        }
        owner.backend.debug_suspend();
        owner.refused_undo(token);
    }

    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(b'U', timing, ControllerKind::ThreeButton, &[]));
        let original = frontend(&owner.backend);
        let token = owner.call();
        let called = frontend(&owner.backend);
        let host = session(&mut owner.backend);
        let EmuBackend::Md(md) = &mut owner.backend else {
            unreachable!()
        };
        assert!(md.try_warm_reset().is_err());
        assert_eq!(frontend(&owner.backend), called);
        assert_eq!(session(&mut owner.backend), host);
        owner.undo(token);
        assert_eq!(frontend(&owner.backend), original);
    }
}

use super::*;
use zeff_pce_core::hardware::{PceCartridgeHardware, PceHuCardBoard};

pub(crate) fn netplay_fixture_hucard() -> Vec<u8> {
    fixture_hucard(false)
}

fn fixture_hucard(populous_ram: bool) -> Vec<u8> {
    let mut code = vec![0x78, 0xD8, 0xD4, 0xA9, 0xFF, 0x53, 1, 0xA9, 0xF8, 0x53, 2];
    if populous_ram {
        code.extend_from_slice(&[0xA9, 0x40, 0x53, 4]);
    }
    fn store(code: &mut Vec<u8>, address: u16, value: u8) {
        code.extend_from_slice(&[0xA9, value, 0x8D]);
        code.extend_from_slice(&address.to_le_bytes());
    }
    for (register, value) in [
        (5, 0x0080_u16),
        (11, 31),
        (12, 0x0F02),
        (13, 0x00EF),
        (14, 4),
    ] {
        code.extend_from_slice(&[3, register, 0x13, value as u8, 0x23, (value >> 8) as u8]);
    }
    for address in [0_u16, 256] {
        store(&mut code, 0x0402, address as u8);
        store(&mut code, 0x0403, (address >> 8) as u8);
        store(&mut code, 0x0404, 0xFF);
        store(&mut code, 0x0405, 1);
    }
    store(&mut code, 0x0800, 0);
    store(&mut code, 0x0801, 0xFF);
    store(&mut code, 0x0804, 0x1F);
    store(&mut code, 0x0805, 0xFF);
    code.extend_from_slice(&[0xA2, 0, 0x8A, 0x8D, 6, 8, 0xE8, 0xE0, 32, 0xD0, 0xF7]);
    store(&mut code, 0x0802, 37);
    store(&mut code, 0x0803, 0);
    store(&mut code, 0x0804, 0x9F);
    let loop_address = 0xE000 + code.len() as u16;
    // Reset the multitap scan, then read each player's button and direction nibbles.
    store(&mut code, 0x1000, 3);
    store(&mut code, 0x1000, 1);
    code.extend_from_slice(&[0xAD, 0, 0x10, 0x8D, 1, 0x20]);
    store(&mut code, 0x1000, 0);
    code.extend_from_slice(&[0xAD, 0, 0x10, 0x8D, 0, 0x20]);
    store(&mut code, 0x1000, 1);
    code.extend_from_slice(&[0xAD, 0, 0x10, 0x8D, 3, 0x20]);
    store(&mut code, 0x1000, 0);
    code.extend_from_slice(&[0xAD, 0, 0x10, 0x8D, 2, 0x20]);
    if populous_ram {
        code.extend_from_slice(&[0xEE, 0, 0x40, 0xD0, 3, 0xEE, 1, 0x40]);
    }
    code.extend_from_slice(&[0xEE, 4, 0x20, 0x4C]);
    code.extend_from_slice(&loop_address.to_le_bytes());
    let mut rom = vec![0xEA; 0x2000];
    rom[..code.len()].copy_from_slice(&code);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

fn backend(hardware: PceCartridgeHardware, board: PceHuCardBoard) -> PceBackend {
    let mut rom = fixture_hucard(board == PceHuCardBoard::Populous);
    if board == PceHuCardBoard::Populous {
        rom.resize(0x80000, 0xEA);
    }
    let mut backend = PceBackend::new_with_overrides(
        rom,
        "synthetic-netplay.pce".into(),
        None,
        Some(board),
        Some(hardware),
    )
    .unwrap();
    backend.configure_netplay_controllers().unwrap();
    backend.set_sample_rate(48_000);
    backend.set_display_config(PceOverscanMode::Full, PcePaletteMode::RawRgb);
    backend
}

#[derive(Debug, PartialEq)]
struct Witness {
    native: Vec<u8>,
    runtime: Vec<u8>,
    rgb: Vec<u8>,
    ram: Vec<u8>,
    persistent: Vec<u8>,
}

fn witness(backend: &PceBackend) -> Witness {
    Witness {
        native: backend.encode_state_bytes().unwrap(),
        runtime: backend.netplay_runtime_state_bytes(),
        rgb: backend.framebuffer().to_vec(),
        ram: backend.machine.mapped_work_ram().to_vec(),
        persistent: backend.netplay_persistent_state_bytes(),
    }
}

#[test]
fn pce_netplay_prediction_correction_preserves_rgb_pcm_ram_and_native_format() {
    for hardware in [PceCartridgeHardware::Base, PceCartridgeHardware::SuperGrafx] {
        for board in [PceHuCardBoard::Plain, PceHuCardBoard::Populous] {
            let mut reference = backend(hardware, board);
            let mut predicted = backend(hardware, board);
            let initial_persistent = predicted.netplay_persistent_state_bytes();
            let reference_lease = reference.begin_netplay_rollback().unwrap();
            let predicted_lease = predicted.begin_netplay_rollback().unwrap();
            assert_eq!(
                reference_lease
                    .advance_frame(&mut reference, [0, 0])
                    .unwrap(),
                predicted_lease
                    .advance_frame(&mut predicted, [0, 0])
                    .unwrap()
            );
            let snapshot = predicted_lease.capture(&predicted).unwrap();
            let before = witness(&predicted);
            if board == PceHuCardBoard::Populous {
                assert!(before.persistent.iter().any(|byte| *byte != 0));
                assert_ne!(before.persistent, initial_persistent);
            }
            assert_eq!(snapshot.checkpoint(), before.native);
            let (header, state) = snapshot.native_state_parts();
            let mut parts = header.to_vec();
            parts.extend_from_slice(state);
            assert_eq!(parts, predicted.encode_state_bytes().unwrap());
            assert_eq!(snapshot.frame, snapshot.core.frame());
            assert_eq!(&before.native[..8], BACKEND_STATE_MAGIC);
            assert_eq!(
                u32::from_le_bytes(before.native[8..12].try_into().unwrap()),
                1
            );
            assert!(snapshot.retained_bytes() < 5 * 1024 * 1024);
            let expected = reference_lease
                .advance_frame(&mut reference, [0x11, 0x22])
                .unwrap();
            predicted_lease
                .advance_frame(&mut predicted, [0x80, 0x08])
                .unwrap();
            if board == PceHuCardBoard::Populous {
                assert_ne!(
                    predicted.netplay_persistent_state_bytes(),
                    before.persistent
                );
            }
            predicted_lease.restore(&mut predicted, &snapshot).unwrap();
            assert_eq!(witness(&predicted), before);
            assert_eq!(snapshot.native_state_parts().0, header);
            assert!(std::ptr::eq(snapshot.native_state_parts().1, state));
            assert_eq!(
                predicted.netplay_persistent_state_bytes(),
                before.persistent
            );
            let actual = predicted_lease
                .advance_frame(&mut predicted, [0x11, 0x22])
                .unwrap();
            assert_eq!(actual, expected);
            assert!(actual.iter().any(|sample| *sample != 0.0));
            assert!(
                reference
                    .framebuffer()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[..3] != [0, 0, 0])
            );
            assert_eq!(witness(&predicted), witness(&reference));
            let corrected = predicted_lease.capture(&predicted).unwrap();
            let (header, state) = corrected.native_state_parts();
            let mut parts = header.to_vec();
            parts.extend_from_slice(state);
            assert_eq!(parts, predicted.encode_state_bytes().unwrap());
            assert_eq!(
                std::array::from_fn::<_, 4, _>(
                    |index| predicted.machine.mapped_work_ram()[index] & 15
                ),
                [0xE, 0xD, 0xD, 0x7]
            );
            assert_ne!(predicted.machine.mapped_work_ram()[4], 0);
            if board == PceHuCardBoard::Populous {
                let persistent = predicted.netplay_persistent_state_bytes();
                assert!(persistent.iter().any(|byte| *byte != 0));
                assert_ne!(persistent, before.persistent);
            }
        }
    }
}

#[test]
fn pce_netplay_foreign_stale_checkpoint_failure_and_recovery() {
    let mut first = backend(PceCartridgeHardware::Base, PceHuCardBoard::Plain);
    let mut other = backend(PceCartridgeHardware::Base, PceHuCardBoard::Plain);
    let lease = first.begin_netplay_rollback().unwrap();
    let initial = lease.capture(&first).unwrap();
    let checkpoint = first.encode_state_bytes().unwrap();
    assert!(first.begin_netplay_rollback().is_err());
    assert!(lease.capture(&other).is_err());
    assert!(lease.capture(&first).is_err());
    lease
        .restore_after_session(&mut first, &initial, &checkpoint)
        .unwrap();
    let other_lease = other.begin_netplay_rollback().unwrap();
    let foreign = other_lease.capture(&other).unwrap();
    assert!(lease.restore(&mut first, &foreign).is_err());
    lease
        .restore_after_session(&mut first, &initial, &checkpoint)
        .unwrap();
    let mut bad = checkpoint.clone();
    bad[12] ^= 1;
    assert!(
        lease
            .restore_after_session(&mut first, &initial, &bad)
            .is_err()
    );
    assert!(lease.advance_frame(&mut first, [0, 0]).is_err());
    lease
        .restore_after_session(&mut first, &initial, &checkpoint)
        .unwrap();
    assert_eq!(first.encode_state_bytes().unwrap(), checkpoint);
    first.machine.debug_suspend();
    assert!(lease.advance_frame(&mut first, [0, 0]).is_err());
    first.machine.debug_continue();
    lease
        .restore_after_session(&mut first, &initial, &checkpoint)
        .unwrap();
    drop(lease);
    let replacement = first.begin_netplay_rollback().unwrap();
    assert!(replacement.restore(&mut first, &initial).is_err());
}

#[test]
fn pce_netplay_admission_rejects_extra_controllers_and_runtime_mutation() {
    let mut backend = backend(PceCartridgeHardware::Base, PceHuCardBoard::Plain);
    backend.update_controller_mode(PceControllerMode::TwoButton);
    assert!(backend.begin_netplay_rollback().is_err());
    backend.update_controller_mode(PceControllerMode::Multitap);
    assert!(backend.begin_netplay_rollback().is_err());
    backend.configure_netplay_controllers().unwrap();
    let lease = backend.begin_netplay_rollback().unwrap();
    backend.set_sample_rate(44_100);
    assert!(lease.capture(&backend).is_err());
    backend.set_sample_rate(48_000);
    assert!(lease.capture(&backend).is_err());
    assert!(backend.begin_netplay_rollback().is_ok());
}

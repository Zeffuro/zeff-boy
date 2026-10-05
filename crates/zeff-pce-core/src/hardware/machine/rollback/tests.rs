use super::*;
use crate::hardware::{FivePortMultitap, PceCartridgeHardware, TwoButtonPad};

fn machine(hardware: PceCartridgeHardware) -> PceMachine {
    let mut rom = vec![0xEA; 0x2000];
    rom[..12].copy_from_slice(&[
        0xD4, 0xA9, 0xF8, 0x53, 2, 0xEE, 0, 0x20, 0x4C, 5, 0xE0, 0xEA,
    ]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let controller = ControllerPort::new(ControllerDevice::Multitap(FivePortMultitap::new([
        MultitapDevice::TwoButton(TwoButtonPad::new()),
        MultitapDevice::TwoButton(TwoButtonPad::new()),
        MultitapDevice::Disconnected,
        MultitapDevice::Disconnected,
        MultitapDevice::Disconnected,
    ])));
    let mut machine = PceMachine::with_cartridge_and_controller(
        rom,
        PceCartridgeDescriptor::default().with_required_hardware(hardware),
        controller,
    )
    .unwrap();
    machine.set_sample_rate(48_000);
    machine
}

#[test]
fn owned_snapshots_restore_both_topologies_without_reallocating_rom() {
    for hardware in [PceCartridgeHardware::Base, PceCartridgeHardware::SuperGrafx] {
        let mut core = machine(hardware);
        let lease = core.begin_rollback_session().unwrap();
        lease.advance_frame(&mut core, [0x41, 0x22]).unwrap();
        let snapshot = lease.capture(&core).unwrap();
        let checkpoint = encode_state(&core).unwrap();
        let runtime = core.encode_rollback_runtime_state();
        let pointer = core.hucard_rom().as_ptr();
        let expected = lease.advance_frame(&mut core, [0x14, 0x88]).unwrap();
        let future = encode_state(&core).unwrap();
        lease.restore(&mut core, &snapshot).unwrap();
        assert_eq!(pointer, core.hucard_rom().as_ptr());
        assert_eq!(checkpoint, encode_state(&core).unwrap());
        assert_eq!(runtime, core.encode_rollback_runtime_state());
        assert_eq!(
            expected,
            lease.advance_frame(&mut core, [0x14, 0x88]).unwrap()
        );
        assert_eq!(future, encode_state(&core).unwrap());
        assert!(core.work_ram()[0] != 0);
    }
}

#[test]
fn incomplete_frame_retires_lease_and_owned_checkpoint_recovers() {
    let mut core = machine(PceCartridgeHardware::Base);
    let lease = core.begin_rollback_session().unwrap();
    let snapshot = lease.capture(&core).unwrap();
    let checkpoint = encode_state(&core).unwrap();
    core.master_ticks = u64::MAX - 1;
    assert!(lease.advance_frame(&mut core, [0, 0]).is_err());
    assert!(lease.capture(&core).is_err());
    lease
        .restore_after_session(&mut core, &snapshot, &checkpoint)
        .unwrap();
    assert_eq!(encode_state(&core).unwrap(), checkpoint);
    assert!(lease.advance_frame(&mut core, [0, 0]).is_ok());
}

#[test]
fn native_restore_keeps_v3_and_cannot_reuse_an_active_lease() {
    let mut core = machine(PceCartridgeHardware::Base);
    let lease = core.begin_rollback_session().unwrap();
    let snapshot = lease.capture(&core).unwrap();
    let checkpoint = encode_state(&core).unwrap();
    assert_eq!(
        &checkpoint[..8],
        super::super::super::save_state::PCE_SAVE_STATE_MAGIC
    );
    assert_eq!(u32::from_le_bytes(checkpoint[8..12].try_into().unwrap()), 3);
    super::super::super::save_state::decode_state(&mut core, &checkpoint).unwrap();
    assert!(lease.capture(&core).is_err());
    assert!(core.begin_rollback_session().is_err());
    lease
        .restore_after_session(&mut core, &snapshot, &checkpoint)
        .unwrap();
    assert!(lease.capture(&core).is_ok());
}

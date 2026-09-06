use super::super::{CD_USER_SECTOR_BYTES, CdDisc, CdTrack, CdTrackMode};
use super::*;

#[cfg(feature = "profiling")]
#[test]
fn coalesced_timing_collapses_ordinary_cpu_action_device_entries() {
    let mut scalar = PceMachine::new(high_speed_loop_rom()).unwrap();
    let mut coalesced = PceMachine::new(high_speed_loop_rom()).unwrap();
    scalar.set_device_advancement_coalescing_for_test(false);
    coalesced.set_device_advancement_coalescing_for_test(true);
    scalar.reset_profiling();
    coalesced.reset_profiling();

    for _ in 0..128 {
        assert_eq!(
            scalar.step_boundary().unwrap(),
            coalesced.step_boundary().unwrap()
        );
    }

    assert_eq!(scalar.debug_snapshot(), coalesced.debug_snapshot());
    assert_eq!(
        scalar.devices().debug_snapshot(),
        coalesced.devices().debug_snapshot()
    );
    assert_eq!(scalar.framebuffer(), coalesced.framebuffer());
    assert!(
        coalesced.profiling_snapshot().device_advance_calls
            < scalar.profiling_snapshot().device_advance_calls
    );
}

fn assert_coalesced_timing_matches_scalar(
    mut scalar: PceMachine,
    mut coalesced: PceMachine,
    boundaries: usize,
) {
    scalar.set_device_advancement_coalescing_for_test(false);
    for _ in 0..boundaries {
        assert_eq!(
            scalar.step_boundary().unwrap(),
            coalesced.step_boundary().unwrap()
        );
        assert_eq!(scalar.debug_snapshot(), coalesced.debug_snapshot());
        assert_eq!(
            scalar.devices().debug_snapshot(),
            coalesced.devices().debug_snapshot()
        );
        assert_eq!(scalar.framebuffer(), coalesced.framebuffer());
        assert_eq!(
            crate::hardware::save_state::encode_state(&scalar).unwrap(),
            crate::hardware::save_state::encode_state(&coalesced).unwrap()
        );
        assert_eq!(
            drain_machine_audio(&mut scalar),
            drain_machine_audio(&mut coalesced)
        );
    }
}

fn timing_observable_program() -> Vec<u8> {
    vec![
        0xA9, 0x00, 0x8D, 0x00, 0x00, // VDC select
        0xA9, 0x12, 0x8D, 0x02, 0x00, // VDC data low
        0xA9, 0x34, 0x8D, 0x03, 0x00, // VDC data high
        0xA9, 0x01, 0x8D, 0x00, 0x04, // VCE
        0xA9, 0xFF, 0x8D, 0x00, 0x08, // PSG
        0xA9, 0x03, 0x8D, 0x00, 0x10, // controller
        0xA9, 0x00, 0x8D, 0x00, 0x18, // CD-ROM2 register
        0xA9, 0x7F, 0x8D, 0x00, 0x0C, // timer reload
        0xA9, 0x01, 0x8D, 0x01, 0x0C, // timer control
        0xAD, 0x00, 0x0C, // timer counter
        0xAD, 0x03, 0x14, // IRQ request
        0xAD, 0x00, 0x00, // VDC status
        0xAD, 0x00, 0x08, // PSG
        0xAD, 0x00, 0x10, // controller
        0xAD, 0x00, 0x18, // CD-ROM2 status
        0x4C, 0x00, 0x20, // JMP $2000
    ]
}

fn configure_timing_observable_program(machine: &mut PceMachine) {
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    machine.cpu_mut().cpu_mut().set_mapping_register(1, 0);
    machine.cpu_mut().cpu_mut().registers_mut().pc = 0x2000;
}

fn timing_observable_hucard_machine() -> PceMachine {
    let mut machine = PceMachine::new(rom_with_program(&timing_observable_program())).unwrap();
    configure_timing_observable_program(&mut machine);
    machine
}

fn timing_observable_cd_machine() -> PceMachine {
    let mut system_card = vec![0xEA; super::super::SYSTEM_CARD_V1_V2_IMAGE_LEN];
    system_card[..timing_observable_program().len()].copy_from_slice(&timing_observable_program());
    let disc = CdDisc::new(vec![
        CdTrack::from_index1_data(
            1,
            0,
            None,
            0,
            CdTrackMode::Mode1_2048,
            vec![0; CD_USER_SECTOR_BYTES],
        )
        .unwrap(),
    ])
    .unwrap();
    let mut machine = PceMachine::with_cdrom2(system_card, disc).unwrap();
    configure_timing_observable_program(&mut machine);
    machine
}

#[test]
fn coalesced_device_timing_matches_scalar_for_mmio_and_on_chip_timer_irq() {
    assert_coalesced_timing_matches_scalar(
        timing_observable_hucard_machine(),
        timing_observable_hucard_machine(),
        28,
    );
}

#[test]
fn coalesced_device_timing_matches_scalar_for_cdrom2_register_mmio() {
    assert_coalesced_timing_matches_scalar(
        timing_observable_cd_machine(),
        timing_observable_cd_machine(),
        20,
    );
}

#[test]
fn coalesced_device_timing_matches_scalar_for_direct_vdc_dma_contention() {
    fn machine() -> PceMachine {
        let mut machine = PceMachine::new(rom_with_program(&[0x13, 0x34, 0x23, 0x12])).unwrap();
        write_vdc_register(machine.devices_mut(), VdcRegister::VramData, 0);
        machine.devices_mut().vdc_mut().vram_mut()[0x0100..0x0120].fill(0xBEEF);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaSource, 0x0100);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaDestination, 0x0200);
        write_vdc_register(machine.devices_mut(), VdcRegister::DmaLength, 31);
        machine
            .devices_mut()
            .vdc_mut()
            .write_port(VdcPort::SelectOrStatus, VdcRegister::VramData as u8);
        machine
    }

    assert_coalesced_timing_matches_scalar(machine(), machine(), 12);
}

#[test]
fn coalesced_device_timing_matches_scalar_for_vdc_irq_acknowledgement() {
    fn machine() -> PceMachine {
        let mut machine = PceMachine::new(rom_with_program(&[0xAD, 0x00, 0x00, 0xEA])).unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
        write_vdc_register(machine.devices_mut(), VdcRegister::Control, 0x0004);
        machine
            .devices_mut()
            .vdc_mut()
            .latch_status(VdcStatus::RASTER_MATCH);
        machine
    }

    assert_coalesced_timing_matches_scalar(machine(), machine(), 8);
}

#[test]
fn coalesced_device_timing_matches_scalar_for_supergrafx_video_mmio() {
    fn machine() -> PceMachine {
        let mut machine = PceMachine::with_supergrafx_substrate_for_test(rom_with_program(&[
            0xA9, 0x01, 0x8D, 0x08, 0x00, // VPC
            0xA9, 0x02, 0x8D, 0x10, 0x00, // VDC2
            0x4C, 0x00, 0xE0,
        ]))
        .unwrap();
        machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
        machine
    }

    assert_coalesced_timing_matches_scalar(machine(), machine(), 24);
}

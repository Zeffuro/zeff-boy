use super::*;
#[cfg(feature = "profiling")]
use crate::hardware::{CD_USER_SECTOR_BYTES, CdDisc, CdTrack, CdTrackMode};

fn plain_memory_program() -> Vec<u8> {
    vec![
        0xA9, 0xF8, 0x53, 0x02, // TAM #$02
        0xA9, 0x5A, 0x8D, 0x00, 0x20, // STA $2000
        0xEE, 0x00, 0x20, // INC $2000
        0xAD, 0x00, 0x20, // LDA $2000
        0xA9, 0x00, 0x8D, 0x00, 0x00, // VDC
        0xA9, 0x7F, 0x8D, 0x00, 0x0C, // timer reload
        0xA9, 0x01, 0x8D, 0x01, 0x0C, // timer control
        0xAD, 0x00, 0x0C, // timer read
        0xAD, 0x03, 0x14, // IRQ request
        0xD4, 0x80, 0xD5, // CSH; BRA loop
    ]
}

fn plain_memory_machine() -> PceMachine {
    let mut machine = PceMachine::new(rom_with_program(&plain_memory_program())).unwrap();
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    machine
}

fn plain_memory_dma_machine() -> PceMachine {
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

#[cfg(feature = "profiling")]
fn plain_memory_cd_machine() -> PceMachine {
    let mut system_card = vec![0xEA; super::super::SYSTEM_CARD_V1_V2_IMAGE_LEN];
    system_card[..plain_memory_program().len()].copy_from_slice(&plain_memory_program());
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
    machine.cpu_mut().cpu_mut().set_mapping_register(0, 0xFF);
    machine
}

fn assert_plain_memory_lane_matches_generic(mut generic: PceMachine, mut lane: PceMachine) {
    generic.set_plain_memory_lane_for_test(false);
    lane.set_plain_memory_lane_for_test(true);
    for _ in 0..2 {
        assert_eq!(
            generic.run_until_frame().unwrap(),
            lane.run_until_frame().unwrap()
        );
        assert_eq!(generic.debug_snapshot(), lane.debug_snapshot());
        assert_eq!(
            generic.devices().debug_snapshot(),
            lane.devices().debug_snapshot()
        );
        assert_eq!(generic.framebuffer(), lane.framebuffer());
        assert_eq!(
            crate::hardware::save_state::encode_state(&generic).unwrap(),
            crate::hardware::save_state::encode_state(&lane).unwrap()
        );
        assert_eq!(
            drain_machine_audio(&mut generic),
            drain_machine_audio(&mut lane)
        );
    }
}

#[test]
fn plain_memory_lane_matches_generic_for_mpr_wram_dummy_timer_irq_and_vdc() {
    assert_plain_memory_lane_matches_generic(plain_memory_machine(), plain_memory_machine());
}

#[test]
fn plain_memory_lane_matches_generic_during_vdc_dma_contention() {
    assert_plain_memory_lane_matches_generic(
        plain_memory_dma_machine(),
        plain_memory_dma_machine(),
    );
}

#[cfg(feature = "profiling")]
#[test]
fn plain_memory_lane_profiles_direct_and_fallback_accesses() {
    let mut machine = plain_memory_machine();
    machine.set_plain_memory_lane_for_test(true);
    machine.reset_profiling();
    machine.run_until_frame().unwrap();
    let snapshot = machine.profiling_snapshot();
    assert!(snapshot.plain_memory_lane_direct_accesses != 0);
    assert!(snapshot.plain_memory_lane_fallback_accesses != 0);
    assert_eq!(
        snapshot.plain_memory_lane_attempts,
        snapshot.plain_memory_lane_direct_accesses + snapshot.plain_memory_lane_fallback_accesses
    );
}

#[cfg(feature = "profiling")]
#[test]
fn plain_memory_lane_rejects_trace_history_cd_and_supergrafx() {
    let mut history = plain_memory_machine();
    history.set_plain_memory_lane_for_test(true);
    history.set_opcode_history_enabled(true);
    history.reset_profiling();
    history.run_until_frame().unwrap();
    assert_eq!(history.profiling_snapshot().plain_memory_lane_attempts, 0);

    let mut trace = plain_memory_machine();
    trace.set_plain_memory_lane_for_test(true);
    trace.set_instruction_trace_enabled(true);
    trace.reset_profiling();
    trace.run_until_frame().unwrap();
    assert_eq!(trace.profiling_snapshot().plain_memory_lane_attempts, 0);

    let mut watchpoint = plain_memory_machine();
    watchpoint.set_plain_memory_lane_for_test(true);
    watchpoint.add_watchpoint_range(RESET_PC, RESET_PC, WatchType::Read);
    watchpoint.reset_profiling();
    watchpoint.run_until_frame().unwrap();
    assert_eq!(
        watchpoint.profiling_snapshot().plain_memory_lane_attempts,
        0
    );

    let mut breakpoint = plain_memory_machine();
    breakpoint.set_plain_memory_lane_for_test(true);
    breakpoint.add_breakpoint(RESET_PC);
    breakpoint.reset_profiling();
    breakpoint.run_until_frame().unwrap();
    assert_eq!(
        breakpoint.profiling_snapshot().plain_memory_lane_attempts,
        0
    );

    let mut cd = plain_memory_cd_machine();
    cd.set_plain_memory_lane_for_test(true);
    cd.reset_profiling();
    cd.run_until_frame().unwrap();
    assert_eq!(cd.profiling_snapshot().plain_memory_lane_attempts, 0);

    let mut supergrafx =
        PceMachine::with_supergrafx_substrate_for_test(high_speed_loop_rom()).unwrap();
    supergrafx.set_plain_memory_lane_for_test(true);
    supergrafx.reset_profiling();
    supergrafx.run_until_frame().unwrap();
    assert_eq!(
        supergrafx.profiling_snapshot().plain_memory_lane_attempts,
        0
    );
}

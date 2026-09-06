use super::*;

fn active_emulator() -> Emulator {
    let mut emulator = Emulator::from_rom_data(&rom_with_reset_code(&[
        0x90, // nop
        0x40, // inc ax
        0x48, // dec ax
        0xEB, 0xFB, // jmp 0000
    ]))
    .unwrap();
    for offset in 0..64_u32 {
        emulator
            .bus
            .write8(offset, (offset as u8).wrapping_mul(29).wrapping_add(3));
    }
    emulator.io_write8(0x0080, 0x20);
    emulator.io_write8(0x0081, 0x03);
    emulator.io_write8(0x0088, 0xF1);
    emulator.io_write8(0x0090, 0x01);
    emulator.io_write8(0x0091, 0x0F);
    emulator
}

fn assert_machine_equal(eager: &mut Emulator, deferred: &mut Emulator) {
    assert_eq!(deferred.cpu_registers(), eager.cpu_registers());
    assert_eq!(deferred.cpu_segments(), eager.cpu_segments());
    assert_eq!(deferred.cpu_ip(), eager.cpu_ip());
    assert_eq!(deferred.cpu_flags(), eager.cpu_flags());
    assert_eq!(deferred.cpu_cycles(), eager.cpu_cycles());
    assert_eq!(deferred.cpu_state(), eager.cpu_state());
    assert_eq!(deferred.last_fetch(), eager.last_fetch());
    assert_eq!(deferred.last_trap(), eager.last_trap());
    assert_eq!(deferred.bus.cycles, eager.bus.cycles);
    assert_eq!(deferred.ppu_debug_snapshot(), eager.ppu_debug_snapshot());
    assert_eq!(deferred.apu_debug_snapshot(), eager.apu_debug_snapshot());
    assert_eq!(deferred.bus.apu.save_state(), eager.bus.apu.save_state());
    assert_eq!(deferred.uart_debug_snapshot(), eager.uart_debug_snapshot());
    assert_eq!(deferred.system_ram(), eager.system_ram());
    assert_eq!(deferred.framebuffer(), eager.framebuffer());
    assert_eq!(
        deferred.encode_state().unwrap(),
        eager.encode_state().unwrap()
    );
    assert_eq!(
        deferred.apu_master_debug_samples_ordered(),
        eager.apu_master_debug_samples_ordered()
    );
    for channel in 0..4 {
        assert_eq!(
            deferred.apu_channel_debug_samples_ordered(channel),
            eager.apu_channel_debug_samples_ordered(channel)
        );
    }
    let mut eager_audio = Vec::new();
    let mut deferred_audio = Vec::new();
    eager.drain_audio_samples_into(&mut eager_audio);
    deferred.drain_audio_samples_into(&mut deferred_audio);
    assert_eq!(deferred_audio, eager_audio);
}

#[test]
fn active_frames_match_eager_service_and_return_materialized() {
    let mut eager = active_emulator();
    let mut deferred = eager.clone();

    for _ in 0..4 {
        eager.eager_service_step_frame();
        deferred.deferred_service_step_frame();
        assert_machine_equal(&mut eager, &mut deferred);
        assert_eq!(deferred.bus.frame_service_state_for_test(), (false, 0, 0));
    }

    #[cfg(feature = "profiling")]
    {
        let snapshot = deferred.profiling_snapshot();
        assert!(snapshot.frame_service_deferred_calls > 1_000);
        assert!(snapshot.apu_step_calls < snapshot.bus_step_calls / 4);
    }
}

#[test]
fn deferred_frame_state_roundtrip_continues_identically() {
    let mut source = active_emulator();
    source.deferred_service_step_frame();
    source.drain_audio_samples_into(&mut Vec::new());
    let state = source.encode_state().unwrap();
    let mut restored = Emulator::from_rom_data(source.cartridge_rom_bytes()).unwrap();
    source.load_state(&state).unwrap();
    restored.load_state(&state).unwrap();

    source.deferred_service_step_frame();
    restored.deferred_service_step_frame();
    assert_machine_equal(&mut source, &mut restored);
}

#[test]
fn hlt_transition_and_interrupt_wake_match_eager_service() {
    let mut eager = Emulator::from_rom_data(&rom_with_reset_code(&[
        0xFB, // sti
        0x90, // interrupt shadow
        0xF4, // hlt
        0xEB, 0xFD, // jmp hlt
    ]))
    .unwrap();
    eager.bus.write16(7 * 4, 0x0003);
    eager.bus.write16(7 * 4 + 2, 0xF000);
    eager.io_write8(0x00B0, 0);
    eager.io_write8(0x00B2, 0x80);
    eager.io_write8(0x00A4, 1);
    eager.io_write8(0x00A5, 0);
    eager.io_write8(0x00A2, 0x01);
    let mut deferred = eager.clone();

    eager.eager_service_step_frame();
    deferred.deferred_service_step_frame();
    assert_machine_equal(&mut eager, &mut deferred);
    assert_eq!(deferred.bus.frame_service_state_for_test(), (false, 0, 0));
}

#[cfg(feature = "profiling")]
#[test]
fn instruction_trace_forces_eager_frame_service() {
    let mut emulator = active_emulator();
    emulator.set_instruction_trace_enabled(true);
    emulator.deferred_service_step_frame();

    assert!(!emulator.instruction_trace().is_empty());
    assert_eq!(emulator.profiling_snapshot().frame_service_frames, 0);
    assert_eq!(emulator.bus.frame_service_state_for_test(), (false, 0, 0));
}

#[test]
fn public_instruction_step_never_leaves_pending_device_time() {
    let mut emulator = active_emulator();
    let _ = emulator.step_instruction();
    assert_eq!(emulator.bus.frame_service_state_for_test(), (false, 0, 0));
    assert_eq!(emulator.cpu_cycles(), emulator.bus.cycles);
}

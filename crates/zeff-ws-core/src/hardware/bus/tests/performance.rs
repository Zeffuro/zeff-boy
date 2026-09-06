use super::super::*;
#[cfg(feature = "profiling")]
use crate::hardware::constants::CYCLES_PER_SCANLINE;

#[cfg(feature = "profiling")]
#[test]
fn profiling_counts_device_calls_and_transitions() {
    let mut bus = Bus::new(super::minimal_cart());
    let cycles = CYCLES_PER_SCANLINE * 144;

    bus.step_cycles(cycles);

    let snapshot = bus.profiling_snapshot();
    assert_eq!(snapshot.bus_step_calls, 1);
    assert_eq!(snapshot.master_cycles, u64::from(cycles));
    assert_eq!(snapshot.uart_step_calls, 1);
    assert_eq!(snapshot.apu_step_calls, 1);
    assert_eq!(snapshot.sound_dma_step_calls, 1);
    assert_eq!(snapshot.ppu_step_calls, 1);
    assert_eq!(snapshot.completed_scanlines, 144);
    assert_eq!(snapshot.vblank_starts, 1);
    assert_eq!(snapshot.hblank_timer_advances, 144);
    assert_eq!(snapshot.vblank_timer_advances, 1);

    bus.reset_profiling();
    assert_eq!(bus.profiling_snapshot(), ProfilingSnapshot::default());
}

#[test]
fn halted_cpu_boundary_stops_before_tied_ppu_and_sound_dma_events() {
    let mut bus = Bus::new(super::color_cart());
    bus.ppu.set_timing_state(0, 128, false);
    bus.write8(0x1234, 0x5A);
    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1234);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 1);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0);
    bus.io_write8(SOUND_DMA_CONTROL_PORT, SOUND_DMA_ENABLE | 0x03);

    assert_eq!(bus.halted_cpu_next_event_cycles(), 128);
    bus.step_cycles(127);
    assert_eq!(bus.halted_cpu_next_event_cycles(), 1);
    assert_eq!(bus.ppu.vcount(), 0);
    assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0);

    bus.step_cycles(1);
    assert_eq!(bus.ppu.vcount(), 1);
    assert_eq!(bus.io_read8(SOUND_VOLUME_CHANNEL2_PORT), 0x5A);
}

#[test]
fn halted_cpu_boundary_preserves_tied_uart_completion_cycle() {
    let mut bus = Bus::new(super::minimal_cart());
    bus.io_write8(
        SERIAL_CONTROL_PORT,
        SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
    );
    bus.io_write8(SERIAL_DATA_PORT, 0xA6);
    bus.step_cycles(799);
    bus.ppu.set_timing_state(9, 255, false);

    assert_eq!(bus.halted_cpu_next_event_cycles(), 1);
    let completed_cycle = bus.cycles + 1;
    bus.step_cycles(1);

    assert_eq!(bus.ppu.vcount(), 10);
    let event = bus.take_wonder_swan_link_tx_event().unwrap();
    assert_eq!(event.byte, 0xA6);
    assert_eq!(event.completed_cycle, completed_cycle);
}

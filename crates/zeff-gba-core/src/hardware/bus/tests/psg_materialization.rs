use super::*;
use zeff_emu_common::save_state::StateWriter;

pub(super) fn display_dma_bus(vblank: bool, destination: u32, value: u16) -> Bus {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.apu.set_deferred_psg_for_test(false);
    for (address, value) in [
        (0x0400_0084, 0x0080),
        (0x0400_0080, 0xFF77),
        (0x0400_0068, 0xF080),
        (0x0400_006C, 0x87F0),
        (0x0400_0082, 0x330F),
        (0x0400_0088, 0xC200),
        (0x0400_00A0, 0x3217),
        (0x0400_00A4, 0x7654),
        (0x0400_0200, 0x3FFF),
        (0x0400_0208, 1),
        (0x0400_0004, 0x38),
    ] {
        bus.write16(address, value);
    }
    if vblank {
        bus.step_cycles(1232 * 159 + 1006);
    }
    bus.step_cycles(bus.ppu.cycles_until_next_status_event() - 64);

    let timing = if vblank { 0x1000 } else { 0x2000 };
    bus.write16(0x0200_0000, value);
    bus.write32(0x0400_00B0, 0x0200_0000);
    bus.write32(0x0400_00B4, destination);
    bus.write16(0x0400_00B8, 1);
    bus.write16(0x0400_00BA, 0xC000 | timing);

    for index in 0..8u32 {
        bus.write32(0x0200_0100 + index * 4, 0x3175_29D3 ^ index);
    }
    bus.write32(0x0400_00BC, 0x0200_0100);
    bus.write32(0x0400_00C0, 0x0400_00A0);
    bus.write16(0x0400_00C4, 4);
    bus.write16(0x0400_00C6, 0xF640);

    bus.write32(0x0400_00C8, 0x0200_0100);
    bus.write32(0x0400_00CC, 0x0400_00A4);
    bus.write16(0x0400_00D0, 1);
    bus.write16(0x0400_00D2, 0xC400 | timing);

    bus.write32(0x0200_0200, 0x00C0_FFE0);
    bus.write32(0x0400_00D4, 0x0200_0200);
    bus.write32(0x0400_00D8, 0x0400_0100);
    bus.write16(0x0400_00DC, 1);
    bus.write16(0x0400_00DE, 0xC400 | timing);
    bus.debug_trace_enabled = true;
    bus.debug_trace_reads = true;
    bus.debug_trace_writes = true;
    bus
}

pub(super) fn assert_bus_equal(eager: &mut Bus, deferred: &mut Bus) {
    assert_eq!(eager.io, deferred.io);
    assert_eq!(eager.ewram, deferred.ewram);
    assert_eq!(eager.iwram, deferred.iwram);
    assert_eq!(eager.ppu.state(), deferred.ppu.state());
    assert_eq!(eager.ppu.framebuffer(), deferred.ppu.framebuffer());
    assert_eq!(
        eager
            .timer_registers_snapshot()
            .map(|timer| (timer.reload, timer.counter, timer.control)),
        deferred.timer_registers_snapshot().map(|timer| (
            timer.reload,
            timer.counter,
            timer.control
        ))
    );
    assert_eq!(eager.timer_timing_state(), deferred.timer_timing_state());
    for (first, second) in eager
        .dma
        .channels()
        .into_iter()
        .zip(deferred.dma.channels())
    {
        assert_eq!(
            (
                first.source,
                first.destination,
                first.count,
                first.control,
                first.active_source,
                first.active_destination,
                first.active_count,
                first.data_latch
            ),
            (
                second.source,
                second.destination,
                second.count,
                second.control,
                second.active_source,
                second.active_destination,
                second.active_count,
                second.data_latch
            )
        );
    }
    assert_eq!(eager.pending_dma_cycles, deferred.pending_dma_cycles);
    assert_eq!(eager.irq_delay_state(), deferred.irq_delay_state());
    assert_eq!(eager.master_cycles, deferred.master_cycles);
    assert_eq!(
        *eager.debug_trace_events.borrow(),
        *deferred.debug_trace_events.borrow()
    );
    assert_eq!(eager.apu.save_state(), deferred.apu.save_state());
    let mut first = StateWriter::new();
    let mut second = StateWriter::new();
    eager.apu.write_psg_state(&mut first);
    deferred.apu.write_psg_state(&mut second);
    assert_eq!(first.into_bytes(), second.into_bytes());
}

#[test]
fn deferred_psg_preserves_display_dma_audio_timer_fifo_and_irq_order() {
    for vblank in [false, true] {
        for (destination, value) in [
            (0x0400_0068, 0x81C0),
            (0x0400_006C, 0xC7F8),
            (0x0400_0080, 0x2277),
            (0x0400_0082, 0xFB0F),
            (0x0400_0084, 0),
            (0x0400_0088, 0x0200),
            (0x0400_0090, 0x17E3),
        ] {
            let mut eager = display_dma_bus(vblank, destination, value);
            let mut deferred = eager.clone();
            deferred.apu.set_deferred_psg_for_test(true);
            eager.step_cycles(13);
            deferred.step_cycles(13);
            assert_ne!(deferred.apu.deferred_psg_pending_t_cycles_for_test(), 0);
            assert_bus_equal(&mut eager, &mut deferred);
            eager.step_cycles(51);
            deferred.step_cycles(51);
            assert_bus_equal(&mut eager, &mut deferred);
            assert!(eager.pending_dma_cycles > 0);
            let interrupts = eager.read16(0x0400_0202);
            assert_eq!(interrupts, deferred.read16(0x0400_0202));
            assert_ne!(interrupts & (1 << 3), 0);
            for cycles in [1, 3, 17, 256, 1232] {
                eager.step_cycles(cycles);
                deferred.step_cycles(cycles);
                assert_bus_equal(&mut eager, &mut deferred);
            }
            let mut first = Vec::new();
            let mut second = Vec::new();
            eager.apu.drain_samples_into(&mut first);
            deferred.apu.drain_samples_into(&mut second);
            assert!(!first.is_empty());
            assert_eq!(
                first
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>(),
                second
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }
}

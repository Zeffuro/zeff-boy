use super::psg_materialization::{assert_bus_equal, display_dma_bus};
use super::*;

#[test]
fn frame_service_defers_whole_walks_and_projects_guest_observations() {
    let mut eager = Bus::new(cartridge(), 48_000);
    eager.write16(0x0400_0100, 0x8000);
    eager.write16(0x0400_0102, 0x0080);
    let mut deferred = eager.clone();
    deferred.begin_frame_service();
    for _ in 0..96 {
        eager.step_cycles(1);
        deferred.step_cycles(1);
        for address in [
            0x0400_0004,
            0x0400_0006,
            0x0400_0084,
            0x0400_0100,
            0x0400_0102,
        ] {
            assert_eq!(eager.read16(address), deferred.read16(address));
        }
    }
    assert_eq!(deferred.frame_service_stats_for_test(), (true, 96, 96));
    assert_eq!(deferred.master_cycles, 0);
    // MMIO writes observe current timer and PPU phases.
    eager.write16(0x0400_0008, 0x0003);
    deferred.write16(0x0400_0008, 0x0003);
    assert_eq!(deferred.frame_service_pending_cycles, 0);
    assert_bus_equal(&mut eager, &mut deferred);
    deferred.end_frame_service();
}

#[test]
fn frame_service_matches_eager_across_partitions_audio_and_mmio() {
    for rate in [1, 44_100, 48_000, 96_000] {
        for resolution in 0..4 {
            let mut eager = Bus::new(sram_cartridge(), rate);
            for (address, value) in [
                (0x0400_0084, 0x0080),
                (0x0400_0080, 0xFF77),
                (0x0400_0060, 0x0069),
                (0x0400_0062, 0xF280),
                (0x0400_0064, 0xC7F8),
                (0x0400_0068, 0xF080),
                (0x0400_006C, 0x87C3),
                (0x0400_0070, 0x0080),
                (0x0400_0072, 0x2060),
                (0x0400_0074, 0xC7F0),
                (0x0400_0078, 0xA300),
                (0x0400_007C, 0xC057),
                (0x0400_0082, 0x330F),
                (0x0400_0088, 0x0200 | (resolution << 14)),
                (0x0400_0100, 0xFF00),
                (0x0400_0102, 0x0080),
                (0x0400_0104, 0xFFFD),
                (0x0400_0106, 0x0084),
            ] {
                eager.write16(address, value);
            }
            let mut deferred = eager.clone();
            deferred.begin_frame_service();
            let mut random = 0xACE1_2345u32;
            for index in 0..4096 {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                let cycles = if index % 31 == 0 {
                    517
                } else {
                    (random >> 24) % 13 + 1
                };
                eager.step_cycles(cycles);
                deferred.step_cycles(cycles);
                assert_eq!(eager.read32(0x0400_0100), deferred.read32(0x0400_0100));
                assert_eq!(eager.read32(0x0400_0004), deferred.read32(0x0400_0004));
                assert_eq!(eager.read16(0x0400_0084), deferred.read16(0x0400_0084));
                if index % 103 == 0 {
                    for (address, value) in [
                        (0x0400_006C, (random as u16 & 0x07FF) | 0x8000),
                        (0x0400_0082, (random as u16 & 0x770F) | 0x0800),
                        (0x0400_00A0, random as u16),
                        (0x0400_0090, (random >> 16) as u16),
                    ] {
                        eager.write16(address, value);
                        deferred.write16(address, value);
                    }
                }
                if index % 257 == 0 {
                    let mut observed = deferred.clone();
                    observed.end_frame_service();
                    assert_bus_equal(&mut eager, &mut observed);
                }
            }
            deferred.end_frame_service();
            assert!(deferred.frame_service_deferred_calls > 1000);
            assert_bus_equal(&mut eager, &mut deferred);
            let mut first = Vec::new();
            let mut second = Vec::new();
            eager.apu.drain_samples_into(&mut first);
            deferred.apu.drain_samples_into(&mut second);
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

#[test]
fn frame_service_preserves_original_display_dma_and_audio_crossing_chunk() {
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
            eager.apu.set_deferred_psg_for_test(true);
            let mut deferred = eager.clone();
            deferred.begin_frame_service();
            for cycles in [3, 4, 6, 51, 1, 3, 17, 256, 1232] {
                eager.step_cycles(cycles);
                deferred.step_cycles(cycles);
                let mut observed = deferred.clone();
                observed.end_frame_service();
                assert_bus_equal(&mut eager, &mut observed);
            }
            deferred.end_frame_service();
            assert_bus_equal(&mut eager, &mut deferred);
        }
    }
}

#[test]
fn frame_service_fences_irq_sampling_and_clocked_cartridges() {
    let mut eager = Bus::new(cartridge(), 48_000);
    eager.write16(0x0400_0200, 1);
    eager.write16(0x0400_0208, 1);
    eager.request_interrupt(1);
    let mut deferred = eager.clone();
    deferred.begin_frame_service();
    for _ in 0..12 {
        assert_eq!(eager.interrupt_ready(), deferred.interrupt_ready());
        eager.step_cycles(1);
        deferred.step_cycles(1);
    }
    deferred.end_frame_service();
    assert_bus_equal(&mut eager, &mut deferred);
    for media in [eeprom_cartridge(), emerald_cartridge()] {
        let mut bus = Bus::new(media, 48_000);
        bus.begin_frame_service();
        for _ in 0..32 {
            bus.step_cycles(1);
        }
        assert_eq!(bus.frame_service_stats_for_test(), (false, 0, 0));
        assert_eq!(bus.master_cycles, 32);
        bus.end_frame_service();
    }
}

#[test]
fn frame_cpu_run_observes_mmio_mutations_without_a_pending_prefix() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.begin_frame_service();
    for (address, value) in [
        (0x0400_0082, 0x330F),
        (0x0400_0100, 0xFFF0),
        (0x0400_0102, 0x00C0),
        (0x0400_0200, 0x0078),
        (0x0400_0208, 1),
    ] {
        bus.begin_frame_cpu_run();
        assert!(bus.frame_cpu_run_can_continue());
        assert_eq!(bus.frame_service_pending_cycles, 0);
        bus.write16(address, value);
        assert!(!bus.frame_cpu_run_can_continue());
    }
    bus.end_frame_service();
}

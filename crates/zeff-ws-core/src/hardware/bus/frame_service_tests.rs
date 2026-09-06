use super::*;
use crate::hardware::cartridge::compute_footer_checksum;

fn color_cart(rtc: bool) -> Cartridge {
    let mut rom = vec![0xFF; 0x10000];
    let footer = rom.len() - 10;
    rom[footer + 1] = 1;
    rom[footer + 4] = 1;
    rom[footer + 7] = u8::from(rtc);
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    Cartridge::load(&rom).unwrap()
}

fn assert_bus_equal(mut eager: Bus, mut deferred: Bus) {
    assert_eq!(deferred.cycles, eager.cycles);
    assert_eq!(deferred.ram, eager.ram);
    assert_eq!(deferred.io, eager.io);
    assert_eq!(deferred.internal_eeprom, eager.internal_eeprom);
    assert_eq!(deferred.ppu.debug_snapshot(), eager.ppu.debug_snapshot());
    assert_eq!(deferred.ppu.framebuffer(), eager.ppu.framebuffer());
    assert_eq!(
        deferred.ppu.sprite_cache_state(),
        eager.ppu.sprite_cache_state()
    );
    assert_eq!(deferred.apu.save_state(), eager.apu.save_state());
    assert_eq!(deferred.apu.debug_snapshot(), eager.apu.debug_snapshot());
    assert_eq!(deferred.uart.save_state(), eager.uart.save_state());
    assert_eq!(
        deferred.sound_dma_save_values(),
        eager.sound_dma_save_values()
    );
    assert_eq!(deferred.rtc_save_state(), eager.rtc_save_state());
    assert_eq!(
        deferred.deferred_linear_bank_save_values(),
        eager.deferred_linear_bank_save_values()
    );
    assert_eq!(deferred.cartridge.bank0(), eager.cartridge.bank0());
    assert_eq!(deferred.cartridge.bank1(), eager.cartridge.bank1());
    assert_eq!(deferred.cartridge.ram_bank(), eager.cartridge.ram_bank());
    assert_eq!(
        deferred.cartridge.linear_bank(),
        eager.cartridge.linear_bank()
    );
    assert_eq!(deferred.cartridge.save_data(), eager.cartridge.save_data());
    assert_eq!(
        deferred.apu.master_debug_samples_ordered(),
        eager.apu.master_debug_samples_ordered()
    );
    for channel in 0..4 {
        assert_eq!(
            deferred.apu.channel_debug_samples_ordered(channel),
            eager.apu.channel_debug_samples_ordered(channel)
        );
    }
    let mut eager_audio = Vec::new();
    let mut deferred_audio = Vec::new();
    eager.apu.drain_audio_samples_into(&mut eager_audio);
    deferred.apu.drain_audio_samples_into(&mut deferred_audio);
    assert_eq!(deferred_audio, eager_audio);
}

fn configure_active_apu(bus: &mut Bus) {
    for offset in 0..64_u32 {
        bus.write8(offset, (offset as u8).wrapping_mul(29).wrapping_add(7));
    }
    for channel in 0..4_u16 {
        let period = 0x0320_u16 + channel * 0x0137;
        bus.io_write8(0x0080 + channel * 2, period as u8);
        bus.io_write8(0x0081 + channel * 2, (period >> 8) as u8);
        bus.io_write8(0x0088 + channel, 0xF1 - channel as u8);
    }
    bus.io_write8(0x008C, 3);
    bus.io_write8(0x008D, 1);
    bus.io_write8(0x008E, 0x15);
    bus.io_write8(0x0092, 0xA5);
    bus.io_write8(0x0093, 0x21);
    bus.io_write8(0x0090, 0xCF);
    bus.io_write8(0x0091, 0x0F);
}

fn configure_sound_dma(bus: &mut Bus, accumulator: u32) {
    for offset in 0..32_u32 {
        bus.write8(0x1200 + offset, (offset as u8).wrapping_mul(37));
    }
    bus.io_write16(SOUND_DMA_SOURCE_LO_PORT, 0x1200);
    bus.io_write16(SOUND_DMA_SOURCE_SEGMENT_PORT, 0);
    bus.io_write16(SOUND_DMA_LENGTH_LO_PORT, 32);
    bus.io_write16(SOUND_DMA_LENGTH_SEGMENT_PORT, 0);
    bus.io_write8(SOUND_DMA_CONTROL_PORT, SOUND_DMA_ENABLE | 0x03);
    bus.load_sound_dma_save_values(0x1200, 32, accumulator);
}

#[test]
fn deferred_prefix_preserves_audio_dma_ppu_overshoot_chunk() {
    let mut eager = Bus::new(color_cart(false));
    configure_active_apu(&mut eager);
    configure_sound_dma(&mut eager, 64);
    eager.ppu.set_timing_state(0, 192, false);
    eager.io_write8(IRQ_ENABLE_PORT, IRQ_LINE_COMPARE | IRQ_HBLANK_TIMER);
    eager.io_write8(LINE_COMPARE_PORT, 1);
    eager.io_write8(TIMER_CONTROL_PORT, 0x03);
    eager.io_write8(HBLANK_TIMER_RELOAD_LO_PORT, 1);
    let mut deferred = eager.clone();

    eager.step_cycles(31);
    eager.step_cycles(32);
    eager.step_cycles(4);

    deferred.begin_frame_service();
    deferred.step_cycles(31);
    deferred.step_cycles(32);
    assert_eq!(deferred.frame_service_state_for_test().1, 63);
    assert_eq!(deferred.ppu.vcount(), 0);
    assert_eq!(deferred.apu.debug_snapshot().buffered_samples, 0);
    deferred.step_cycles(4);
    deferred.end_frame_service();

    assert_bus_equal(eager, deferred);
}

#[test]
fn deferred_uart_completion_keeps_exact_cycle_and_irq_state() {
    let mut eager = Bus::new(color_cart(false));
    eager.apu.set_sample_generation_enabled(false);
    eager.io_write8(IRQ_ENABLE_PORT, IRQ_SERIAL_TX);
    eager.io_write8(
        SERIAL_CONTROL_PORT,
        SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
    );
    eager.io_write8(SERIAL_DATA_PORT, 0xA6);
    eager.ppu.set_timing_state(0, 100, false);
    let mut deferred = eager.clone();
    let chunks = [17_u32, 29, 3, 41, 5, 11, 7, 23];

    let mut elapsed = 0_u32;
    let mut index = 0_usize;
    while elapsed < 800 {
        let chunk = chunks[index % chunks.len()].min(800 - elapsed);
        eager.step_cycles(chunk);
        elapsed += chunk;
        index += 1;
    }

    deferred.begin_frame_service();
    elapsed = 0;
    index = 0;
    while elapsed < 800 {
        let chunk = chunks[index % chunks.len()].min(800 - elapsed);
        deferred.step_cycles(chunk);
        elapsed += chunk;
        index += 1;
    }
    deferred.end_frame_service();

    let event = deferred.uart.save_state().completed_tx_events[0];
    assert_eq!(event.completed_cycle, 800);
    assert_ne!(deferred.io[usize::from(IRQ_STATUS_PORT)] & IRQ_SERIAL_TX, 0);
    assert_bus_equal(eager, deferred);
}

#[test]
fn io_and_zero_cycle_calls_materialize_pending_time() {
    let mut eager = Bus::new(color_cart(false));
    configure_active_apu(&mut eager);
    let mut deferred = eager.clone();

    eager.step_cycles(19);
    eager.io_write8(0x008C, 0xFE);
    eager.step_cycles(0);
    eager.step_cycles(13);

    deferred.begin_frame_service();
    deferred.step_cycles(19);
    assert_eq!(deferred.frame_service_state_for_test().1, 19);
    deferred.io_write8(0x008C, 0xFE);
    assert_eq!(deferred.frame_service_state_for_test().1, 0);
    deferred.step_cycles(0);
    deferred.step_cycles(13);
    deferred.end_frame_service();

    assert_bus_equal(eager, deferred);
}

#[test]
fn randomized_partition_and_mutation_schedule_matches_eager() {
    let mut eager = Bus::new(color_cart(true));
    configure_active_apu(&mut eager);
    configure_sound_dma(&mut eager, 17);
    eager.io_write8(IRQ_ENABLE_PORT, 0xF1);
    eager.io_write8(
        SERIAL_CONTROL_PORT,
        SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD,
    );
    eager.io_write8(SERIAL_DATA_PORT, 0x5A);
    let mut deferred = eager.clone();
    deferred.begin_frame_service();
    let mut seed = 0xC001_D00D_u32;

    for iteration in 0..2_000_u32 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let cycles = (seed >> 29) + 1;
        eager.step_cycles(cycles);
        deferred.step_cycles(cycles);
        if iteration % 17 == 0 {
            let address = (seed >> 8) & 0xFFFF;
            let value = seed as u8;
            eager.write8(address, value);
            deferred.write8(address, value);
        }
        if iteration % 53 == 0 {
            let port = if iteration & 1 == 0 {
                0x0088
            } else {
                LINE_COMPARE_PORT
            };
            let value = (seed >> 16) as u8;
            eager.io_write8(port, value);
            deferred.io_write8(port, value);
        }
        if iteration % 97 == 0 {
            assert_eq!(
                deferred.io_read8(CURRENT_LINE_PORT),
                eager.io_read8(CURRENT_LINE_PORT)
            );
        }
    }
    deferred.end_frame_service();

    assert_bus_equal(eager, deferred);
}

#[cfg(feature = "profiling")]
#[test]
fn profiling_distinguishes_requests_from_materializations() {
    let mut bus = Bus::new(color_cart(false));
    bus.begin_frame_service();
    for _ in 0..63 {
        bus.step_cycles(1);
    }
    bus.step_cycles(1);
    bus.end_frame_service();

    let snapshot = bus.profiling_snapshot();
    assert_eq!(snapshot.bus_step_calls, 64);
    assert_eq!(snapshot.master_cycles, 64);
    assert_eq!(snapshot.frame_service_frames, 1);
    assert_eq!(snapshot.frame_service_deferred_calls, 63);
    assert_eq!(snapshot.frame_service_deferred_cycles, 63);
    assert_eq!(snapshot.frame_service_materializations, 1);
    assert_eq!(snapshot.frame_service_horizon_crossings, 1);
    assert_eq!(snapshot.frame_service_max_pending_cycles, 63);
    assert_eq!(snapshot.apu_step_calls, 2);
    assert_eq!(snapshot.ppu_step_calls, 2);
}

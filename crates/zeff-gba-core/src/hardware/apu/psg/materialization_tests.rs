use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn seeded_apu() -> Apu {
    let mut apu = Apu::new(48_000);
    for (address, value) in [
        (NR52, 0x80),
        (NR50, 0x77),
        (NR51, 0xFF),
        (NR10, 0x11),
        (NR11, 0x80),
        (NR12, 0xF3),
        (NR13, 0x87),
        (NR14, 0xC3),
        (NR21, 0x40),
        (NR22, 0x92),
        (NR23, 0xD1),
        (NR24, 0xC5),
        (NR30, 0x80),
        (NR31, 0x0D),
        (NR32, 0x20),
        (NR33, 0xA7),
        (NR34, 0xC7),
        (NR41, 0x20),
        (NR42, 0xB2),
        (NR43, 0x19),
        (NR44, 0xC0),
    ] {
        apu.write_psg(address, value);
    }
    for offset in 0..16 {
        apu.write_psg(WAVE_RAM_START + offset, (offset as u8).wrapping_mul(37));
    }
    for value in 0..16u16 {
        apu.write_fifo_halfword(0, value.wrapping_mul(0x1703));
        apu.write_fifo_halfword(1, value.wrapping_mul(0x0317));
    }
    apu
}

fn pair(apu: Apu) -> (Apu, Apu) {
    let mut eager = apu.clone();
    eager.set_deferred_psg_for_test(false);
    let mut deferred = apu;
    deferred.set_deferred_psg_for_test(true);
    (eager, deferred)
}

fn psg_bytes(apu: &Apu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    apu.write_psg_state(&mut writer);
    writer.into_bytes()
}

fn bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|sample| sample.to_bits()).collect()
}

fn assert_equal(eager: &Apu, deferred: &Apu) {
    assert_eq!(psg_bytes(eager), psg_bytes(deferred));
    assert_eq!(eager.save_state(), deferred.save_state());
    assert_eq!(bits(&eager.sample_buffer), bits(&deferred.sample_buffer));
    assert_eq!(eager.debug_snapshot(), deferred.debug_snapshot());
    assert_eq!(eager.psg_regs_snapshot(), deferred.psg_regs_snapshot());
    assert_eq!(
        eager.psg_wave_ram_snapshot(),
        deferred.psg_wave_ram_snapshot()
    );
    assert_eq!(eager.psg_nr52_raw(), deferred.psg_nr52_raw());
    for channel in 0..4 {
        assert_eq!(
            bits(&eager.psg_channel_debug_samples_ordered(channel)),
            bits(&deferred.psg_channel_debug_samples_ordered(channel))
        );
    }
    for fifo in 0..2 {
        assert_eq!(
            bits(&eager.direct_debug_samples_ordered(fifo)),
            bits(&deferred.direct_debug_samples_ordered(fifo))
        );
    }
    assert_eq!(
        bits(&eager.psg_master_debug_samples_ordered()),
        bits(&deferred.psg_master_debug_samples_ordered())
    );
    assert_eq!(
        bits(&eager.master_debug_samples_ordered()),
        bits(&deferred.master_debug_samples_ordered())
    );
    for address in (NR10..=NR52).chain(WAVE_RAM_START..=WAVE_RAM_END) {
        assert_eq!(eager.read_psg(address), deferred.read_psg(address));
    }
}

fn advance_pair(eager: &mut Apu, deferred: &mut Apu, cycles: u32, bias: u16) {
    eager.step_output(cycles, 0x330F, 0x80, bias);
    deferred.step_output(cycles, 0x330F, 0x80, bias);
    assert_equal(eager, deferred);
}

#[test]
fn deferred_psg_preserves_original_chunks_crossing_sequencer_and_sweep() {
    for remainder in 0..4 {
        for boundary in 0..3 {
            let mut apu = seeded_apu();
            apu.psg_cycle_accum = remainder;
            apu.psg.ch1_sweep_pending_disable_delay = 0;
            apu.psg.ch1_sweep_trigger_visibility_delay = 0;
            apu.psg.frame_seq_cycle_accum = 0;
            match boundary {
                0 => {
                    apu.psg.frame_seq_cycle_accum = FRAME_SEQUENCER_PERIOD_CYCLES - 5;
                    apu.psg.channels[1].length_counter = 1;
                    apu.psg.ch2_output_delay = 0;
                    apu.psg.ch2_timer = 4;
                }
                1 => apu.psg.ch1_sweep_pending_disable_delay = 5,
                _ => apu.psg.ch1_sweep_trigger_visibility_delay = 5,
            }
            let (mut eager, mut deferred) = pair(apu);
            for cycles in [3, 5, 7] {
                advance_pair(&mut eager, &mut deferred, cycles, 0x0200);
            }
            assert!(deferred.psg_pending_t_cycles > 0);
            advance_pair(&mut eager, &mut deferred, 44, 0x0200);
            assert_eq!(deferred.psg_pending_t_cycles, 0);
            advance_pair(&mut eager, &mut deferred, 80_003, 0x0200);
        }
    }
}

#[test]
fn deferred_psg_dac_chunk_runs_separately_from_pending_prefix() {
    for resolution in 0..4 {
        for remainder in 0..4 {
            let mut apu = seeded_apu();
            apu.psg_cycle_accum = remainder;
            apu.psg.ch1_sweep_trigger_visibility_delay = 0;
            apu.psg.channels[1].length_counter = 1;
            apu.psg.frame_seq_cycle_accum = FRAME_SEQUENCER_PERIOD_CYCLES - 10;
            let (mut eager, mut deferred) = pair(apu);
            let bias = 0x0200 | (resolution << 14);
            advance_pair(&mut eager, &mut deferred, 19, bias);
            assert!(deferred.psg_pending_t_cycles > 0);
            advance_pair(&mut eager, &mut deferred, 8193, bias);
            assert_eq!(deferred.psg_pending_t_cycles, 0);
            assert!(!eager.sample_buffer.is_empty());
        }
    }
}

#[test]
fn deferred_psg_randomized_partitions_writes_policies_and_observers_match_eager() {
    for sample_rate in [8_000, 44_100, 48_000, 96_000] {
        let (mut eager, mut deferred) = pair(seeded_apu());
        eager.set_sample_rate(sample_rate);
        deferred.set_sample_rate(sample_rate);
        let mut random = 0x21D8_43B7u32;
        for index in 0..1200 {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let cycles = (random >> 24) % 47 + 1;
            let bias = 0x0200 | (((index / 73) & 3) << 14);
            advance_pair(&mut eager, &mut deferred, cycles, bias);
            if index % 13 == 0 {
                let address = [NR11, NR13, NR14, NR23, NR24, NR33, NR34, NR43, NR44]
                    [(random as usize >> 8) % 9];
                let value = (random >> 16) as u8;
                eager.write_psg(address, value);
                deferred.write_psg(address, value);
            }
            if index % 17 == 0 {
                let address = WAVE_RAM_START + (index % 16);
                eager.write_psg(address, random as u8);
                deferred.write_psg(address, random as u8);
            }
            if index % 23 == 0 {
                let timer = usize::from(index & 1);
                assert_eq!(
                    eager.on_timer_overflow(timer, 0x730F),
                    deferred.on_timer_overflow(timer, 0x730F)
                );
                eager.write_fifo_halfword(timer, random as u16);
                deferred.write_fifo_halfword(timer, random as u16);
            }
            if index % 101 == 0 {
                let enabled = index % 202 == 0;
                eager.set_debug_capture_enabled(enabled);
                deferred.set_debug_capture_enabled(enabled);
                assert_eq!(deferred.psg_pending_t_cycles, 0);
            }
            if index % 127 == 0 {
                let enabled = index % 254 != 0;
                eager.set_sample_generation_enabled(enabled);
                deferred.set_sample_generation_enabled(enabled);
            }
            if index % 137 == 0 {
                let muted = index % 274 == 0;
                let mutes = [muted, !muted, false, muted, false, !muted];
                eager.set_channel_mutes(mutes);
                deferred.set_channel_mutes(mutes);
            }
            if index % 139 == 0 {
                let enabled = index % 278 != 0;
                eager.write_psg(NR52, u8::from(enabled) << 7);
                deferred.write_psg(NR52, u8::from(enabled) << 7);
            }
            if index % 149 == 0 {
                let mut first = Vec::new();
                let mut second = Vec::new();
                eager.drain_samples_into(&mut first);
                deferred.drain_samples_into(&mut second);
                assert_eq!(bits(&first), bits(&second));
            }
            assert_equal(&eager, &deferred);
        }
    }
}

#[test]
fn deferred_psg_state_projection_and_restore_discard_old_pending_time() {
    let (mut eager, mut deferred) = pair(seeded_apu());
    advance_pair(&mut eager, &mut deferred, 13, 0x0200);
    let pending = deferred.psg_pending_t_cycles;
    assert_ne!(pending, 0);
    let encoded = psg_bytes(&deferred);
    assert_eq!(encoded.len(), super::state::SAVE_STATE_SIZE);
    assert_eq!(deferred.psg_pending_t_cycles, pending);

    let mut restored = seeded_apu();
    restored.set_deferred_psg_for_test(true);
    restored.step_output(7, 0x330F, 0x80, 0x0200);
    assert_ne!(restored.psg_pending_t_cycles, 0);
    restored.load_save_state(eager.save_state());
    restored
        .read_psg_state(&mut StateReader::new(&encoded))
        .unwrap();
    assert_eq!(restored.psg_pending_t_cycles, 0);
    for cycles in [1, 3, 7, 51, 4093, 8195] {
        advance_pair(&mut eager, &mut restored, cycles, 0x0200);
    }

    deferred.reset_hardware();
    eager.reset_hardware();
    assert_equal(&eager, &deferred);
    assert_eq!(deferred.psg_pending_t_cycles, 0);
}

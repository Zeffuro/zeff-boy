use super::*;
use zeff_gb_core::save_state::decode_on_thread;

#[test]
fn wave_source_reset_uses_hram_stack_and_timer_and_frame_profiles_keep_their_clocks() -> Result<()>
{
    let cancel = AtomicBool::new(false);
    for (bytes, timer) in [
        (zeff_audio_discovery::gb_wave::synthetic_rom(), true),
        (zeff_audio_discovery::gb_wave::synthetic_frame_rom(), false),
    ] {
        let report = zeff_audio_discovery::scan(
            zeff_emu_common::system::System::Gb,
            &bytes,
            Default::default(),
            &cancel,
        );
        let prepared =
            zeff_audio_discovery::gb_wave::prepare_rom(&bytes, &report.gb_wave_songs[0], &cancel)?;
        let mut results = Vec::new();
        for poison in [0x5a, 0xa5] {
            let emulator = Emulator::from_rom_data(&prepared.bytes, HardwareModePreference::Auto)?;
            let mut state = decode_on_thread(emulator.encode_state()?)?;
            state
                .bus
                .cartridge
                .restore_rom_bytes(prepared.bytes.clone());
            state.bus.set_apu_sample_generation_enabled(false);
            for address in 0xc000..0xe000 {
                state.bus.write_byte(address, poison);
            }
            for address in 0xff80..0xffff {
                state.bus.write_byte(address, poison);
            }
            state.cpu.pc = 0x100;
            for _ in 0..1000 {
                if state.cpu.pc == prepared.wait_start {
                    break;
                }
                state.cpu.step(&mut state.bus);
            }
            assert_eq!(state.cpu.pc, prepared.wait_start);
            assert_eq!(state.cpu.sp, 0xfffe);
            assert_eq!(
                state.bus.read_byte(prepared.ready_address),
                prepared.ready_value
            );
            assert_eq!(state.bus.read_byte(prepared.ack_address), 0);
            assert_eq!(state.bus.read_byte(0xff4d) & 0x80, 0x80);
            for _ in 0..60 {
                state.cpu.step(&mut state.bus);
                assert!((prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc));
            }
            assert_eq!(state.bus.read_byte(0xc000), poison);
            state
                .bus
                .write_byte(prepared.ack_address, prepared.ack_value);
            let mut ticks = Vec::new();
            let mut resets = 0;
            let end = state.cpu.cycles + 5_000_000;
            while state.cpu.cycles < end {
                if state.cpu.pc == 0x300 {
                    resets += 1;
                    assert_eq!(state.cpu.sp, 0xfffc);
                }
                if state.cpu.pc == 0x4060 {
                    ticks.push(state.cpu.cycles);
                    assert_eq!(state.bus.read_byte(0xff80), 1);
                }
                state.cpu.step(&mut state.bus);
            }
            assert_eq!(resets, 1);
            assert!(ticks.len() > 30);
            let nominal = if timer { 139264 } else { 140448 };
            assert!(
                ticks
                    .windows(2)
                    .all(|pair| pair[1].abs_diff(pair[0] + nominal) <= 8)
            );
            assert_eq!(state.bus.read_byte(0xc000), ticks.len() as u8);
            assert_eq!(state.bus.read_byte(0xc001), 1);
            assert_eq!(state.bus.read_byte(0xff07) & 7, if timer { 7 } else { 0 });
            assert_eq!(state.bus.read_byte(0xffff), if timer { 4 } else { 1 });
            assert_eq!(state.bus.read_byte(0xff4d) & 0x80, 0x80);
            assert!((0xcef0..=0xcf00).contains(&state.cpu.sp));
            if !timer {
                assert_eq!(state.bus.read_byte(0xc185), 1);
            }
            results.push(ticks.len());
        }
        assert_eq!(results[0], results[1]);
    }
    Ok(())
}

#[test]
fn wave_timer_and_frame_pcm_reset_at_both_rates_and_keep_duration_bound() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for bytes in [
        zeff_audio_discovery::gb_wave::synthetic_rom(),
        zeff_audio_discovery::gb_wave::synthetic_frame_rom(),
    ] {
        let report = zeff_audio_discovery::scan(
            zeff_emu_common::system::System::Gb,
            &bytes,
            Default::default(),
            &cancel,
        );
        let song = &report.gb_wave_songs[0];
        for sample_rate in [44_100, 48_000] {
            let mut session = GbBankedSession::new_wave(
                zeff_audio_discovery::gb_wave::prepare_rom(&bytes, song, &cancel)?,
                RenderOptions {
                    sample_rate,
                    ..options()
                },
                song.warnings.clone(),
                &cancel,
            )?;
            let pcm = render(&mut session, 2048)?;
            assert_eq!(pcm.len(), sample_rate as usize * 2);
            assert!(pcm.iter().any(|sample| *sample != 0));
            session.reset()?;
            assert_eq!(render(&mut session, 258)?, pcm);
        }
        use crate::audio_discovery::{catalog::SongRef, pcm::song::PcmSong};
        let selected = PcmSong::from_ref(SongRef::GbWave(song)).unwrap();
        assert!(
            selected
                .session(
                    &bytes,
                    RenderOptions {
                        max_seconds: 181,
                        ..options()
                    },
                    &cancel
                )
                .is_err()
        );
    }
    Ok(())
}

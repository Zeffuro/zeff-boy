use super::*;
use zeff_gb_core::save_state::decode_on_thread;

#[test]
fn cache_cold_start_keeps_original_interrupts_until_ack_then_isolates_native_service() -> Result<()>
{
    let cancel = AtomicBool::new(false);
    let bytes = zeff_audio_discovery::gb_cache::synthetic_rom();
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let prepared =
        zeff_audio_discovery::gb_cache::prepare_rom(&bytes, &report.gb_cache_songs[0], &cancel)?;
    let mut results = Vec::new();
    for poison in [0x5a, 0xa5] {
        let emulator = Emulator::from_rom_data(&prepared.bytes, HardwareModePreference::ForceDmg)?;
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
        let mut original_irqs = 0;
        for _ in 0..200_000 {
            if state.cpu.pc == prepared.wait_start {
                break;
            }
            if state.cpu.pc == 0x300 {
                original_irqs += 1;
            }
            state.cpu.step(&mut state.bus);
        }
        assert_eq!(state.cpu.pc, prepared.wait_start);
        assert_eq!(original_irqs, 3);
        assert_eq!(state.bus.read_byte(0xc10f), 3);
        assert_eq!(state.bus.read_byte(0xc106), 0x37);
        assert_eq!(state.bus.read_byte(0xc101), 0);
        assert_eq!(state.bus.read_byte(0xdffe), 0);
        assert_eq!(state.cpu.sp, 0xfff0);
        assert_eq!(
            state.bus.read_byte(prepared.ready_address),
            prepared.ready_value
        );
        assert_eq!(state.bus.read_byte(prepared.ack_address), 0);
        assert_eq!(state.bus.read_byte(0xffff), 0);
        for _ in 0..60 {
            state.cpu.step(&mut state.bus);
            assert!((prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc));
        }
        state
            .bus
            .write_byte(prepared.ack_address, prepared.ack_value);
        let end = state.cpu.cycles + 2_000_000;
        let mut ticks = Vec::new();
        while state.cpu.cycles < end {
            assert_ne!(state.cpu.pc, 0x300);
            if state.cpu.pc == 0x4080 {
                ticks.push(state.cpu.cycles);
            }
            state.cpu.step(&mut state.bus);
        }
        assert_eq!(state.bus.read_byte(0xc101), 4);
        assert_eq!(state.bus.read_byte(0xc10f), 3);
        assert_eq!(state.bus.read_byte(0xdffe), 0xa5);
        assert_eq!(state.bus.read_byte(0xc102), ticks.len() as u8);
        assert!(ticks.len() > 20);
        assert!(
            ticks
                .windows(2)
                .all(|pair| pair[1].abs_diff(pair[0] + 70_224) <= 8)
        );
        assert_eq!(state.bus.cartridge.rom_offset(0x4000), Some(0x4000));
        assert!((0xffe0..=0xfff0).contains(&state.cpu.sp));
        results.push(ticks.len());
    }
    assert_eq!(results[0], results[1]);
    Ok(())
}

#[test]
fn cache_pcm_ack_excludes_cold_start_audio_and_reset_is_exact_at_both_rates() -> Result<()> {
    let cancel = AtomicBool::new(false);
    let bytes = zeff_audio_discovery::gb_cache::synthetic_rom();
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let song = &report.gb_cache_songs[0];
    for sample_rate in [44_100, 48_000] {
        let mut session = GbBankedSession::new_cache(
            zeff_audio_discovery::gb_cache::prepare_rom(&bytes, song, &cancel)?,
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
        assert_eq!(session.emulator.cpu_peek8(0xc10f), 3);
        session.reset()?;
        assert_eq!(render(&mut session, 258)?, pcm);
    }
    use crate::audio_discovery::{catalog::SongRef, pcm::song::PcmSong};
    let selected = PcmSong::from_ref(SongRef::GbCache(song)).unwrap();
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
    Ok(())
}

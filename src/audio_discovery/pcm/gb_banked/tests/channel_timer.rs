use super::*;
use zeff_gb_core::save_state::{SaveState, decode_on_thread};

fn registers(state: &SaveState) -> [u8; 8] {
    let r = &state.cpu.regs;
    [r.a, r.f, r.b, r.c, r.d, r.e, r.h, r.l]
}

#[test]
fn channel_native_ram_generation_timer_rearm_and_mapper_restore_survive_poison() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_channel::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let prepared = zeff_audio_discovery::gb_channel::prepare_rom(
        &bytes,
        &report.gb_channel_songs[0],
        &cancel,
    )?;
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
        for _ in 0..20_000 {
            if (prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc) {
                break;
            }
            state.cpu.step(&mut state.bus);
        }
        assert!((prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc));
        assert_eq!(state.bus.read_byte(0xfffc), 0xa5);
        assert_eq!(state.bus.read_byte(0xfffd), 0);
        assert_eq!(state.cpu.sp, 0xc0c0);
        assert_eq!(state.bus.read_byte(0xffff), 0);
        assert_eq!(state.bus.read_byte(0xff07) & 7, 0);
        assert_eq!(
            (0xcb80..0xcb85)
                .map(|a| state.bus.read_byte(a))
                .collect::<Vec<_>>(),
            [0x21, 0x80, 0xc2, 0x34, 0xc9]
        );
        for _ in 0..60 {
            state.cpu.step(&mut state.bus);
            assert!((prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc));
        }
        assert_eq!(state.bus.read_byte(0xc281), 0);
        state.bus.write_byte(0xfffd, 0x5a);
        let end = state.cpu.cycles + 2_000_000;
        let mut entered = None;
        let mut ticks = Vec::new();
        let mut returns = 0;
        while state.cpu.cycles < end {
            if state.cpu.pc == 0x9c {
                entered = Some((registers(&state), state.cpu.sp));
            }
            if state.cpu.pc == 0x450 {
                ticks.push(state.cpu.cycles);
            }
            let returning = state.cpu.pc == 0xc4;
            state.cpu.step(&mut state.bus);
            if returning {
                let (saved, sp) = entered.take().unwrap();
                assert_eq!(registers(&state), saved);
                assert_eq!(state.cpu.sp, sp + 2);
                assert_eq!(state.bus.cartridge.rom_offset(0x4000), Some(6 * 0x4000));
                assert_eq!(state.bus.read_byte(0xc103), 6);
                returns += 1;
            }
        }
        assert!(ticks.len() > 40);
        assert_eq!(ticks.len(), returns);
        assert_eq!(state.bus.read_byte(0xc281), 4);
        assert_eq!(state.bus.read_byte(0xc280), ticks.len() as u8);
        assert_eq!(state.bus.read_byte(0xff07) & 7, 4);
        assert_eq!(state.bus.read_byte(0xffff), 5);
        assert!(ticks.windows(2).all(|pair| {
            [34816, 35840]
                .iter()
                .any(|gap| pair[1].abs_diff(pair[0] + gap) <= 8)
        }));
        results.push(ticks.len());
        state.bus.write_byte(0xffff, 0);
        state.bus.write_byte(0xff0f, 0);
        state.bus.write_byte(0xff07, 0);
        state.bus.write_byte(0xc106, 1);
        state.bus.write_byte(0xff05, 0x33);
        state.bus.write_byte(0xff06, 0x44);
        state.cpu.sp = 0xc0be;
        state.bus.write_byte(0xc0be, 0);
        state.bus.write_byte(0xc0bf, 3);
        state.cpu.pc = u16::from_le_bytes([prepared.bytes[0x41], prepared.bytes[0x42]]);
        state.cpu.running = zeff_gb_core::hardware::types::CpuState::Running;
        state.cpu.ime = zeff_gb_core::hardware::types::ImeState::Disabled;
        let saved = registers(&state);
        for _ in 0..100 {
            state.cpu.step(&mut state.bus);
            if state.cpu.pc == 0x300 {
                break;
            }
        }
        assert_eq!(state.cpu.pc, 0x300);
        assert_eq!(state.cpu.sp, 0xc0c0);
        assert_eq!(registers(&state), saved);
        assert_eq!(state.bus.read_byte(0xff05), 0x33);
        assert_eq!(state.bus.read_byte(0xff06), 0x44);
        assert_eq!(state.bus.read_byte(0xff07) & 7, 0);
    }
    assert_eq!(results[0], results[1]);
    Ok(())
}

#[test]
fn channel_native_pcm_resets_in_dmg_at_both_rates_and_refuses_excess_duration() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_channel::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    for sample_rate in [44_100, 48_000] {
        let song = &report.gb_channel_songs[0];
        let mut session = GbBankedSession::new_channel(
            zeff_audio_discovery::gb_channel::prepare_rom(&bytes, song, &cancel)?,
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
        assert_eq!(session.emulator.hardware_mode(), HardwareMode::DMG);
        session.reset()?;
        assert_eq!(render(&mut session, 258)?, pcm);
    }
    use crate::audio_discovery::{catalog::SongRef, pcm::song::PcmSong};
    let song = PcmSong::from_ref(SongRef::GbChannel(&report.gb_channel_songs[0])).unwrap();
    assert!(
        song.session(
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

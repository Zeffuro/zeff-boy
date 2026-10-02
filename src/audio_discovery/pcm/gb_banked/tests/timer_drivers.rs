use super::*;
use zeff_gb_core::save_state::{SaveState, decode_on_thread};

#[test]
fn resident_and_dynamic_timer_bootstrap_preserve_irq_registers_and_source_clocks() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for (bytes, resident, timer) in [
        (
            zeff_audio_discovery::gb_resident::synthetic_rom(),
            true,
            false,
        ),
        (
            zeff_audio_discovery::gb_resident::synthetic_timer_rom(),
            true,
            true,
        ),
        (zeff_audio_discovery::gb_timer::synthetic_rom(), false, true),
        (
            zeff_audio_discovery::gb_timer::synthetic_mbc2_rom(),
            false,
            true,
        ),
    ] {
        let report = zeff_audio_discovery::scan(
            zeff_emu_common::system::System::Gb,
            &bytes,
            Default::default(),
            &cancel,
        );
        for index in 0..2 {
            let prepared = if resident {
                zeff_audio_discovery::gb_resident::prepare_rom(
                    &bytes,
                    &report.gb_resident_songs[index],
                    &cancel,
                )?
            } else {
                zeff_audio_discovery::gb_timer::prepare_rom(
                    &bytes,
                    &report.gb_timer_songs[index],
                    &cancel,
                )?
            };
            let emulator =
                Emulator::from_rom_data(&prepared.bytes, HardwareModePreference::ForceDmg)?;
            let mut state = decode_on_thread(emulator.encode_state()?)?;
            state.bus.cartridge.restore_rom_bytes(prepared.bytes);
            state.bus.set_apu_sample_generation_enabled(false);
            for _ in 0..10_000 {
                if state.bus.read_byte(prepared.ready_address) == prepared.ready_value
                    && (prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc)
                {
                    break;
                }
                state.cpu.step(&mut state.bus);
            }
            assert_eq!(
                state.bus.read_byte(prepared.ready_address),
                prepared.ready_value
            );
            assert!((prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc));
            assert_eq!(state.bus.read_byte(0xffff), 0);
            assert_eq!(state.bus.read_byte(0xff07) & 7, 0);
            assert_eq!(state.bus.cartridge.rom_offset(0x4000), Some(0x4000));
            assert_eq!(state.cpu.sp, 0xcf00);
            state
                .bus
                .write_byte(prepared.ack_address, prepared.ack_value);
            let end = state.cpu.cycles + 1_000_000;
            let tick = if resident { 0x400 } else { 0x3f2 };
            let mut ticks = Vec::new();
            let mut modulos = Vec::new();
            let mut saved = None;
            let mut returned = 0;
            while state.cpu.cycles < end {
                if state.cpu.pc == tick {
                    ticks.push(state.cpu.cycles);
                    modulos.push(state.bus.read_byte(0xff06));
                }
                if state.cpu.pc == 0x200 {
                    saved = Some((registers(&state), state.cpu.sp));
                }
                let returning = state.cpu.pc == 0x20b;
                state.cpu.step(&mut state.bus);
                if returning {
                    let (registers_before, sp) = saved.take().unwrap();
                    assert_eq!(registers(&state), registers_before);
                    assert_eq!(state.cpu.sp, sp + 2);
                    returned += 1;
                }
            }
            assert!(ticks.len() > 10);
            assert_eq!(returned, ticks.len());
            assert_eq!(state.bus.read_byte(0xffff), if timer { 4 } else { 1 });
            assert_eq!(state.bus.read_byte(0xff07) & 7, if timer { 4 } else { 0 });
            if resident {
                let nominal = if timer { 73_728 } else { 70_224 };
                assert!(
                    ticks
                        .windows(2)
                        .all(|pair| pair[1].abs_diff(pair[0] + nominal) <= 8)
                );
                assert_eq!(
                    state.bus.read_byte(0xce00),
                    report.gb_resident_songs[index].index as u8
                );
            } else {
                assert_eq!(
                    state.bus.read_byte(0xce01),
                    report.gb_timer_songs[index].index as u8
                );
                assert!(modulos.windows(2).any(|pair| pair[0] != pair[1]));
                assert!(
                    ticks
                        .windows(3)
                        .any(|window| window[2] - window[1] != window[1] - window[0])
                );
            }
        }
    }
    Ok(())
}

fn registers(state: &SaveState) -> [u8; 8] {
    let r = &state.cpu.regs;
    [r.a, r.f, r.b, r.c, r.d, r.e, r.h, r.l]
}

#[test]
fn timer_preview_and_audio_export_refuse_unqualified_duration() -> Result<()> {
    use crate::audio_discovery::{catalog::SongRef, pcm::song::PcmSong};
    let cancel = AtomicBool::new(false);
    let bytes = zeff_audio_discovery::gb_timer::synthetic_mbc2_rom();
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let song = PcmSong::from_ref(SongRef::GbTimer(&report.gb_timer_songs[0])).unwrap();
    let qualified = RenderOptions {
        max_seconds: 180,
        ..options()
    };
    assert_eq!(
        song.session(&bytes, qualified, &cancel)?.duration_frames(),
        180 * 44_100
    );
    let excessive = RenderOptions {
        max_seconds: 181,
        ..options()
    };
    assert!(song.session(&bytes, excessive, &cancel).is_err());
    Ok(())
}

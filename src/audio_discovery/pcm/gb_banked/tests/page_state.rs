use super::*;
use zeff_gb_core::save_state::{SaveState, decode_on_thread};

#[test]
fn page_state_bootstrap_clears_cold_state_and_preserves_frame_irq_registers() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for bytes in [
        zeff_audio_discovery::gb_page::synthetic_rom(),
        zeff_audio_discovery::gb_page::synthetic_normal_rom(),
    ] {
        let report = zeff_audio_discovery::scan(
            zeff_emu_common::system::System::Gb,
            &bytes,
            Default::default(),
            &cancel,
        );
        let song = &report.gb_page_songs[0];
        let prepared = zeff_audio_discovery::gb_page::prepare_rom(&bytes, song, &cancel)?;
        let emulator = Emulator::from_rom_data(&prepared.bytes, HardwareModePreference::ForceCgb)?;
        let mut state = decode_on_thread(emulator.encode_state()?)?;
        state.bus.cartridge.restore_rom_bytes(prepared.bytes);
        state.bus.set_apu_sample_generation_enabled(false);
        for address in 0xc000..0xe000 {
            state.bus.write_byte(address, 0xa5);
        }
        for _ in 0..10_000 {
            if state.bus.read_byte(prepared.ready_address) == prepared.ready_value
                && (prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc)
            {
                break;
            }
            state.cpu.step(&mut state.bus);
        }
        assert!((prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc));
        for address in song.state_page..song.state_page + 17 {
            assert_eq!(state.bus.read_byte(address), 0);
        }
        if song.double_speed {
            assert!((0xc680..0xc686).all(|address| state.bus.read_byte(address) == 0));
        }
        for _ in 0..100 {
            state.cpu.step(&mut state.bus);
            assert!((prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc));
        }
        assert_eq!(
            state.bus.read_byte(prepared.ready_address),
            prepared.ready_value
        );
        assert_eq!(state.bus.read_byte(0xffff), 0);
        state
            .bus
            .write_byte(prepared.ack_address, prepared.ack_value);
        let mut saved = None;
        let mut ticks = Vec::new();
        let mut returned = 0;
        let period = if song.double_speed { 140_448 } else { 70_224 };
        let end = state.cpu.cycles + 20 * period;
        while state.cpu.cycles < end {
            if state.cpu.pc == 0x200 {
                saved = Some((registers(&state), state.cpu.sp));
            }
            if state.cpu.pc == 0x480 {
                ticks.push(state.cpu.cycles);
            }
            let returning = state.cpu.pc == 0x20b;
            state.cpu.step(&mut state.bus);
            if returning {
                let (before, sp) = saved.take().unwrap();
                assert_eq!(registers(&state), before);
                assert_eq!(state.cpu.sp, sp + 2);
                returned += 1;
            }
        }
        assert!(ticks.len() >= 19);
        assert_eq!(ticks.len(), returned);
        assert!(
            ticks
                .windows(2)
                .all(|pair| pair[1].abs_diff(pair[0] + period) <= 8)
        );
        assert_eq!(state.bus.read_byte(0xff07) & 7, 0);
        assert_eq!(state.bus.read_byte(0xffff), 1);
        assert_eq!(state.bus.read_byte(0xff4d) & 0x80 != 0, song.double_speed);
        assert_eq!(
            state
                .bus
                .read_byte(if song.double_speed { 0xfffd } else { 0xfffe }),
            0xcd
        );
    }
    Ok(())
}

fn registers(state: &SaveState) -> [u8; 8] {
    let r = &state.cpu.regs;
    [r.a, r.f, r.b, r.c, r.d, r.e, r.h, r.l]
}

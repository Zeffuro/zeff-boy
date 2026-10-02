use super::*;
use zeff_gb_core::save_state::decode_on_thread;

#[test]
fn timed_event_irq_preserves_registers_and_crosses_timestamp_pages() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_timed::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    for song in &report.gb_timed_songs {
        let prepared = zeff_audio_discovery::gb_timed::prepare_rom(&bytes, song, &cancel)?;
        let emulator = Emulator::from_rom_data(&prepared.bytes, HardwareModePreference::ForceDmg)?;
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
        assert!((prepared.wait_start..prepared.wait_end).contains(&state.cpu.pc));
        assert_eq!(state.bus.read_byte(0xffff), 0);
        assert_eq!(state.cpu.sp, 0xcf00);
        state
            .bus
            .write_byte(prepared.ack_address, prepared.ack_value);
        let end = state.cpu.cycles + 600 * 70_224;
        let mut ticks = Vec::new();
        let mut incoming = None;
        let mut returned = 0;
        let mut crossed_page = false;
        let mut restarted_default = false;
        while state.cpu.cycles < end {
            let registers = |s: &zeff_gb_core::save_state::SaveState| {
                let r = &s.cpu.regs;
                [r.a, r.f, r.b, r.c, r.d, r.e, r.h, r.l]
            };
            if state.cpu.pc == 0x200 {
                incoming = Some((registers(&state), state.cpu.sp));
            }
            if state.cpu.pc == 0x400 {
                ticks.push(state.cpu.cycles);
                crossed_page |= state.bus.read_byte(0xce01) >= 1;
                let pointer =
                    u16::from_le_bytes([state.bus.read_byte(0xce03), state.bus.read_byte(0xce04)]);
                restarted_default |= ticks.len() > 300 && (0x4000..0x4100).contains(&pointer);
            }
            let returning = state.cpu.pc == 0x20e;
            state.cpu.step(&mut state.bus);
            if returning {
                let (saved, sp) = incoming.take().unwrap();
                assert_eq!(registers(&state), saved);
                assert_eq!(state.cpu.sp, sp + 2);
                returned += 1;
            }
        }
        assert!(ticks.len() > 550);
        assert_eq!(ticks.len(), returned);
        assert!(crossed_page);
        assert!(restarted_default);
        assert!(
            ticks
                .windows(2)
                .all(|pair| pair[1].abs_diff(pair[0] + 70_224) <= 8)
        );
        assert_eq!(state.bus.read_byte(0xff07) & 7, 0);
        assert_eq!(state.bus.read_byte(0xffff), 1);
    }
    Ok(())
}

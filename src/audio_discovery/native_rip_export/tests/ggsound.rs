use super::*;
use zeff_nes_core::emulator::Emulator;

#[test]
fn returning_nsf_init_preserves_caller_stack_and_resets_owned_state() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for source in [
        zeff_audio_discovery::nes_native::fixture_rom_ggsound(false),
        zeff_audio_discovery::nes_native::fixture_rom_ggsound(true),
    ] {
        let report = zeff_audio_discovery::scan(
            zeff_emu_common::system::System::Nes,
            &source,
            Default::default(),
            &cancel,
        );
        for song in &report.nes_native_songs {
            let nsf = native_rips::encode(&source, SongRef::NesNative(song), &cancel)?;
            let mut rom = vec![0; 0xa010];
            rom[..16].copy_from_slice(&source[..16]);
            rom[16..0x8010].copy_from_slice(&nsf.bytes[0x80..]);
            let play = song.native.tick.canonical_cpu_address as u16;
            let caller = [
                0x78,
                0xd8,
                0xa2,
                0xdf,
                0x9a,
                0x20,
                0,
                0xf8,
                0xa9,
                0x3c,
                0x8d,
                0xef,
                7,
                0x20,
                play as u8,
                (play >> 8) as u8,
                0xa9,
                0x5a,
                0x8d,
                0xee,
                7,
                0x4c,
                0x15,
                0xf7,
            ];
            rom[0x7710..0x7710 + caller.len()].copy_from_slice(&caller);
            rom[0x800c..0x800e].copy_from_slice(&0xf700_u16.to_le_bytes());
            let mut clean_state = None;
            for fill in [0, 0xa5] {
                let mut emulator = Emulator::new(&rom, 48_000.0)?;
                for address in 0..0x800 {
                    emulator.cpu_write8(address, fill);
                }
                for _ in 0..30_000 {
                    if emulator.cpu_pc() == 0xf715 {
                        break;
                    }
                    emulator.step_instruction();
                }
                assert_eq!(emulator.cpu_pc(), 0xf715);
                assert_eq!(emulator.cpu_sp(), 0xdf);
                assert_eq!(emulator.cpu_peek8(0x7ef), 0x3c);
                assert_eq!(emulator.cpu_peek8(0x7ee), 0x5a);
                for address in 0..0x800 {
                    if !(0..0x39).contains(&address)
                        && !(0x100..0x200).contains(&address)
                        && !(0x300..0x38a).contains(&address)
                        && ![0x7ee, 0x7ef].contains(&address)
                    {
                        assert_eq!(emulator.cpu_peek8(address), fill, "address {address:x}");
                    }
                }
                let state: Vec<_> = (0..0x39)
                    .chain(0x300..0x38a)
                    .map(|address| emulator.cpu_peek8(address))
                    .collect();
                if let Some(clean) = &clean_state {
                    assert_eq!(&state, clean);
                } else {
                    clean_state = Some(state);
                }
            }
        }
    }
    Ok(())
}

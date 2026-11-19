use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn write(chip: &mut Opn2, address: u8, value: u8) {
    chip.write_port(0, address);
    chip.write_port(1, value);
}

#[test]
fn both_timer_status_projection_is_nonmutating_at_every_partial_slot() {
    for mode in [0, 2, 10, 15, 0x8f] {
        for wait in 0..16 {
            let mut base = Opn2::default();
            base.advance(wait * 144);
            for (address, value) in [(0x24, 255), (0x25, 3), (0x26, 255), (0x27, mode)] {
                write(&mut base, address, value);
            }
            for cut in 0..144 {
                let mut source = base;
                source.advance(2160 + cut);
                write(&mut source, 0x27, mode | 0x30);
                let saved = source;
                for clocks in [0, 1, 5, 6, 7, 143, 144, 145, 2305] {
                    let status = source.status_after(clocks);
                    assert_eq!(source, saved);
                    let mut actual = source;
                    actual.advance(clocks);
                    assert_eq!(status, actual.status(0), "{mode}/{wait}/{cut}/{clocks}");
                    for port in [1, 2, 3] {
                        assert_eq!(actual.status(port), 0);
                    }
                }
            }
        }
    }
}

#[test]
fn current_state_retains_both_timers_and_csm_at_every_partial_edge() {
    for mode in [15, 0x4f, 0x8f, 0xcf] {
        let mut base = Opn2::default();
        for (address, value) in [(0x24, 255), (0x25, 3), (0x26, 255), (0x27, mode)] {
            write(&mut base, address, value);
        }
        for cut in 0..144 {
            let mut original = base;
            original.advance(2160 + cut);
            write(&mut original, 0x27, mode | 0x30);
            let mut writer = StateWriter::new();
            original.write_state(&mut writer);
            let bytes = writer.into_bytes();
            let mut reader = StateReader::new(&bytes);
            let mut restored = Opn2::read_state(&mut reader).unwrap();
            assert!(reader.is_exhausted());
            assert_eq!(original, restored);
            for clocks in [0, 1, 5, 6, 7, 125, 2305] {
                original.advance(clocks);
                restored.advance(clocks);
                assert_eq!(original, restored);
                assert_eq!(
                    original.sample_with_mutes([false; 6]),
                    restored.sample_with_mutes([false; 6])
                );
            }
        }
    }
}

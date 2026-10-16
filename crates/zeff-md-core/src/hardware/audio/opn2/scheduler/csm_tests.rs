use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn write(chip: &mut Opn2, address: u8, value: u8) {
    chip.write_port(0, address);
    chip.write_port(1, value);
}

fn setup(period: u16, manual: u8) -> Opn2 {
    let mut chip = Opn2::default();
    for group in [0, 4, 8, 12] {
        for (register, value) in [(0x30, 1), (0x40, 63), (0x50, 31), (0x80, 15)] {
            write(&mut chip, register + group + 2, value);
        }
    }
    write(&mut chip, 0xb2, 7);
    write(&mut chip, 0xa6, 0x22);
    write(&mut chip, 0xa2, 0x69);
    let reload = 1024 - period;
    write(&mut chip, 0x24, (reload >> 2) as u8);
    write(&mut chip, 0x25, reload as u8 & 3);
    write(&mut chip, 0x28, manual | 2);
    write(&mut chip, 0x27, 0x81);
    chip
}

#[test]
fn start_pulse_keys_all_four_slots_before_first_overflow() {
    let mut chip = setup(1024, 0);
    chip.advance(6 * 3);
    assert!(chip.timer_a.csm());
    assert!(chip.slot(2).pipeline.csm && chip.slot(2).pipeline.sampled);
    assert!(!chip.slot(2).pipeline.desired);
    chip.advance(6 * 20);
    for slot in [2, 8, 14, 20] {
        assert!(chip.slot(slot).pipeline.csm);
        assert!(chip.slot(slot).control_snapshot().2);
        assert!(!chip.slot(slot).pipeline.desired);
    }
    assert_eq!(chip.flags, 0);
}

#[test]
fn continuous_period_one_csm_does_not_invent_repeated_key_edges() {
    let mut chip = setup(1, 0);
    chip.advance(6 * 23);
    assert!(
        [2, 8, 14, 20]
            .into_iter()
            .all(|slot| chip.slot(slot).pipeline.reset)
    );
    chip.advance(144);
    for slot in [2, 8, 14, 20] {
        assert!(chip.slot(slot).pipeline.sampled && chip.slot(slot).pipeline.csm);
        assert!(!chip.slot(slot).pipeline.reset);
        assert!(!chip.slot(slot).pipeline.desired);
    }
}

#[test]
fn mode_exit_retains_held_and_sampled_csm_until_their_owners_run() {
    for manual in [0, 0xf0] {
        let mut chip = setup(1024, manual);
        chip.advance(18);
        write(&mut chip, 0x27, 1);
        assert!(chip.timer_a.csm());
        chip.advance(6 * 20);
        for slot in [2, 8, 14, 20] {
            assert!(chip.slot(slot).pipeline.csm);
            assert!(chip.slot(slot).control_snapshot().2);
        }
        chip.advance(144);
        for slot in [2, 8, 14, 20] {
            assert!(!chip.slot(slot).pipeline.csm);
            assert_eq!(chip.slot(slot).control_snapshot().2, manual != 0);
            assert_eq!(chip.slot(slot).pipeline.desired, manual != 0);
        }
    }
}

#[test]
fn active_csm_partial_cuts_restore_every_key_timer_and_envelope_owner() {
    for period in [1, 2] {
        for manual in [0, 0xf0] {
            for mode in [0, 0x40, 0x80, 0xc0] {
                for cut in 0..24 {
                    for fraction in 0..6 {
                        let mut chip = setup(period, manual);
                        chip.advance(6 * (96 + cut) + fraction);
                        write(&mut chip, 0x27, mode | 1);
                        write(&mut chip, 0x42, cut as u8 * 5);
                        let mut writer = StateWriter::new();
                        chip.write_state(&mut writer);
                        let bytes = writer.into_bytes();
                        let mut restored = Opn2::read_state(&mut StateReader::new(&bytes)).unwrap();
                        assert_eq!(restored, chip);
                        for clocks in [0, 1, 5, 7, 144, 307] {
                            let saved = chip;
                            assert_eq!(chip.status_after(clocks), {
                                let mut future = chip;
                                future.advance(clocks);
                                future.status(0)
                            });
                            assert_eq!(chip, saved);
                            chip.advance(clocks);
                            for _ in 0..clocks {
                                restored.advance(1);
                            }
                            assert_eq!(chip, restored, "{period}/{manual}/{mode}/{cut}/{fraction}");
                        }
                    }
                }
            }
        }
    }
}

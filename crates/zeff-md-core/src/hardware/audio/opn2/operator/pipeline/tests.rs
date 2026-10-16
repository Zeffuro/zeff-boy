use super::super::super::Opn2;
use super::*;

fn write(chip: &mut Opn2, address: u8, value: u8) {
    chip.write_port(0, address);
    chip.write_port(1, value);
}

#[test]
fn csm_generation_uses_live_mode_but_selected_total_level_and_am() {
    use zeff_emu_common::save_state::{StateReader, StateWriter};
    for slot in 0..24 {
        for initial in [0, 0x40, 0x80, 0xc0] {
            for mode in [0, 0x40, 0x80, 0xc0] {
                for fraction in 0..6 {
                    let mut chip = Opn2::default();
                    let channel = slot % 6;
                    let bank = if channel >= 3 { 2 } else { 0 };
                    chip.write_port(bank, 0xb4 + (channel % 3) as u8);
                    chip.write_port(bank + 1, 0xf0);
                    write(&mut chip, 0x22, 15);
                    write(&mut chip, 0x27, initial);
                    chip.advance(6 * (96 + slot as u64));
                    let operator = chip.slot_mut(slot);
                    operator.attenuation = 211;
                    operator.envelope = Envelope::Sustain;
                    operator.keyed = true;
                    operator.pipeline.desired = true;
                    operator.write(4, 63);
                    operator.write(6, 128);
                    chip.advance(6 + fraction);
                    let selected = chip.slot(slot).pipeline;
                    assert_eq!(selected.total_level, 63);
                    let am = selected.level_add - 504;
                    assert!(am > 0);
                    chip.slot_mut(slot).write(4, 1);
                    write(&mut chip, 0x27, mode);
                    let mut writer = StateWriter::new();
                    chip.write_state(&mut writer);
                    let bytes = writer.into_bytes();
                    let mut restored = Opn2::read_state(&mut StateReader::new(&bytes)).unwrap();
                    assert_eq!(restored, chip);
                    chip.advance(6 - fraction);
                    for _ in 0..6 - fraction {
                        restored.advance(1);
                    }
                    assert_eq!(restored, chip);
                    let expected = 211 + am + if channel == 2 && mode == 0x80 { 0 } else { 504 };
                    assert_eq!(chip.slot(slot).pipeline.level, expected);
                    assert_eq!(chip.slot(slot).pipeline.total_level, 63);
                    let generated = chip.slot(slot).pipeline.level;
                    write(&mut chip, 0x27, mode ^ 0x80);
                    assert_eq!(chip.slot(slot).pipeline.level, generated);
                    chip.advance(1007);
                    write(&mut restored, 0x27, mode ^ 0x80);
                    for _ in 0..1007 {
                        restored.advance(1);
                    }
                    assert_eq!(restored, chip);
                }
            }
        }
    }
}

#[test]
fn total_level_omission_retains_ssg_transform_am_and_saturation() {
    for mode in 0..16 {
        for keyed in [false, true] {
            for level in [0, 211, 511, 512, 1008, 1023] {
                for tl in [0, 1, 63, 127] {
                    for am in [0, 37, 126] {
                        let mut operator = Operator {
                            attenuation: level,
                            envelope: Envelope::Sustain,
                            keyed,
                            ..Operator::default()
                        };
                        operator.write(4, tl);
                        operator.write(6, 128);
                        operator.write(9, mode);
                        operator.pipeline.sampled = keyed;
                        operator.select_rate((0x400, 3), am, true);
                        let envelope = operator.ssg.level(level);
                        operator.write(4, 127 - tl);
                        operator.generate_level(true);
                        assert_eq!(operator.pipeline.level, (envelope + am).min(ENVELOPE_MAX));
                        operator.generate_level(false);
                        assert_eq!(
                            operator.pipeline.level,
                            (envelope + am + u16::from(tl) * 8).min(ENVELOPE_MAX)
                        );
                        assert_eq!(operator.pipeline.total_level, tl);
                    }
                }
            }
        }
    }
}

#[test]
fn parameter_writes_preserve_selected_envelope_and_frequency_actions() {
    let mut chip = Opn2::default();
    write(&mut chip, 0x30, 1);
    write(&mut chip, 0xa4, 0x2f);
    write(&mut chip, 0xa0, 0);
    chip.advance(6 * 96);
    let old_frequency = chip.scheduler.frequency;
    chip.slot_mut(0).attenuation = 64;
    chip.slot_mut(0).envelope = Envelope::Release;
    write(&mut chip, 0x80, 0x2f);
    write(&mut chip, 0x40, 10);
    write(&mut chip, 0xa4, 0x04);
    write(&mut chip, 0xa0, 0x55);
    chip.advance(6);
    let selected = chip.slot(0).pipeline.selected;
    assert_eq!(selected.effective, 63);
    assert_eq!(selected.sustain, 64);
    assert_eq!(chip.slot(0).pipeline.level_add, 80);
    let expected_increment = chip.slot(0).modulated_phase_step(
        old_frequency.0 * 2,
        old_frequency.1,
        key_code(old_frequency.0, old_frequency.1),
    ) & PHASE_MASK;
    assert_eq!(chip.slot(0).pipeline.increment, expected_increment);
    write(&mut chip, 0x80, 0x50);
    write(&mut chip, 0x40, 20);
    chip.advance(6);
    let action = chip.slot(0).pipeline.prepared;
    assert!(action.maximum);
    assert_eq!(action.sustain, 64);
    assert_eq!(chip.slot(0).pipeline.level, 144);
    chip.advance(6);
    assert_eq!(chip.slot(0).attenuation, 64 + u16::from(action.increment));
    chip.advance(6 * 23);
    assert_eq!(chip.slot(0).pipeline.selected.sustain, 160);
    assert!(!chip.slot(0).pipeline.prepared.maximum);
    assert_eq!(chip.slot(0).pipeline.level_add, 160);
    assert_ne!(chip.slot(0).pipeline.increment, expected_increment);
}

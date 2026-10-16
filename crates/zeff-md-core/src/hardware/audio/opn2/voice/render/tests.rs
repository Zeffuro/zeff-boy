use super::*;
use crate::hardware::audio::opn2::{Opn2, operator::Sample};

#[test]
fn prepared_feedback_survives_a_later_feedback_write() {
    let mut channel = Channel {
        history: [513, 514],
        ..Channel::default()
    };
    channel.write_channel(0xb0, 0x3f);
    channel.operators[0].pipeline.sample = Sample {
        phase: 0,
        level: 0,
        valid: true,
    };
    channel.prepare_modulation(0);
    assert_eq!(channel.render.modulation[0], 128);
    channel.write_channel(0xb0, 7);
    channel.render_operator(0);
    assert_eq!(channel.raw_operator(0), (5792, true));
}

#[test]
fn history_consumption_preserves_the_preparation_ages() {
    let mut channel = Channel {
        history: [11, 12],
        previous_s2: 7,
        ..Channel::default()
    };
    channel.render.raw = [31, 23, 5, 0];
    channel.write_channel(0xb0, 1);
    channel.prepare_modulation(2);
    assert_eq!(channel.render.modulation[2], 9);
    channel.consume_history(0);
    assert_eq!(channel.history, [31, 11]);
    channel.write_channel(0xb0, 3);
    channel.prepare_modulation(3);
    assert_eq!(channel.render.modulation[3], 6);
    channel.consume_history(1);
    assert_eq!(channel.previous_s2, 23);
}

#[test]
fn consumed_carriers_keep_their_algorithm_and_completed_value_until_latched() {
    let mut chip = Opn2::default();
    chip.channels[1].render.raw = [3200, 0, 0, 0];
    chip.channels[1].write_channel(0xb1, 7);
    chip.accumulate_slot(1);
    chip.channels[1].write_channel(0xb1, 0);
    for slot in [7, 13, 19] {
        chip.accumulate_slot(slot);
    }
    assert_eq!(chip.scheduler.accumulator[1], 100);
    assert_eq!(chip.scheduler.output[1], 0);
    chip.accumulate_slot(1);
    assert_eq!(chip.scheduler.output[1], 3200);
    assert_eq!(chip.output[1], 0);
    chip.publish_slot(0);
    assert_eq!(chip.output[1], 3200);
}

#[test]
fn render_codec_preserves_invalid_payloads_and_rejects_bad_ranges_or_truncation() {
    let value = Render {
        modulation: [8191, -8191, 1027, -10],
        raw: [-8191, 8191, 0, 31],
        mod_valid: 5,
        raw_valid: 10,
    };
    let mut writer = Writer::new();
    value.write_state(&mut writer);
    let bytes = writer.into_bytes();
    assert_eq!(Render::read_state(&mut Reader::new(&bytes)).unwrap(), value);
    for offset in (0..32).step_by(4) {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&8192_u32.to_le_bytes());
        assert!(Render::read_state(&mut Reader::new(&bad)).is_err());
    }
    for offset in [32, 33] {
        let mut bad = bytes.clone();
        bad[offset] = 16;
        assert!(Render::read_state(&mut Reader::new(&bad)).is_err());
    }
    for end in 0..bytes.len() {
        assert!(Render::read_state(&mut Reader::new(&bytes[..end])).is_err());
    }
}

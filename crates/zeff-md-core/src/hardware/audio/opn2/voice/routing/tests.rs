use super::*;
use crate::hardware::audio::opn2::Opn2;
use crate::hardware::audio::opn2::operator::Pipeline;

fn write(chip: &mut Opn2, address: u8, value: u8) {
    chip.write_port(0, address);
    chip.write_port(1, value);
}

fn tone() -> Opn2 {
    let mut chip = Opn2::default();
    for slot in [0, 4, 8, 12] {
        for (group, value) in [(0x30, 1), (0x50, 31)] {
            write(&mut chip, group + slot, value);
        }
    }
    for (address, value) in [(0xb0, 0x3c), (0xa4, 0x0a), (0xa0, 0x84), (0x28, 0xf0)] {
        write(&mut chip, address, value);
    }
    chip.advance(144 * 12);
    assert_ne!(chip.output[0], 0);
    chip
}

#[test]
fn feedback_write_after_capture_cannot_rewrite_the_rendered_action() {
    let mut early = tone();
    let mut late = early;
    early.advance(6 * 7);
    late.advance(6 * 7);
    write(&mut early, 0xb0, 4);
    early.advance(6 * 17);
    late.advance(6 * 17);
    assert_eq!(early.output, late.output);
    assert_eq!(early.channels[0].history, late.channels[0].history);
    assert_eq!(early.channels[0].render.modulation[0], 0);
    assert_ne!(
        early.channels[0].render.modulation[0],
        late.channels[0].render.modulation[0]
    );
    write(&mut late, 0xb0, 4);
    early.advance(144);
    late.advance(144);
    assert_ne!(early.channels[0].history, late.channels[0].history);
}

#[test]
fn every_pending_routing_byte_is_saved_and_validated() {
    let original = Routing {
        prepared: [63, 4, 3, 39],
        sampled: [7, 59, 0, 44],
        prepared_valid: 5,
        sampled_valid: 10,
    };
    let mut writer = Writer::new();
    original.write_state(&mut writer);
    let saved = writer.into_bytes();
    assert_eq!(
        Routing::read_state(&mut Reader::new(&saved)).unwrap(),
        original
    );
    for offset in 0..saved.len() {
        let mut bad = saved.clone();
        bad[offset] = if offset < 8 { 64 } else { 16 };
        assert!(Routing::read_state(&mut Reader::new(&bad)).is_err());
    }
    for end in 0..saved.len() {
        assert!(Routing::read_state(&mut Reader::new(&saved[..end])).is_err());
    }
}

#[test]
fn slot_state_migration_keeps_operators_and_warms_missing_render_history() {
    let mut old = tone();
    old.advance(6 * 7 + 3);
    write(&mut old, 0xb0, 4);
    old.output = [37, -41, 23, -17, 11, -5];
    let mut writer = Writer::new();
    old.write_state_version(&mut writer, 6);
    let mut restored = Opn2::read_state_version(&mut Reader::new(&writer.into_bytes()), 6).unwrap();
    for slot in 0..24 {
        old.slot_mut(slot).pipeline.total_level = Pipeline::migrated(false).total_level;
    }
    for (before, after) in old.channels.iter().zip(restored.channels) {
        assert_eq!(before.operators, after.operators);
        assert_eq!(before.history, after.history);
        assert_eq!(before.previous_s2, after.previous_s2);
        assert_eq!(after.render, Render::migrated());
    }
    assert_eq!(restored.scheduler.step, old.scheduler.step);
    assert_eq!(restored.scheduler.command, old.scheduler.command);
    assert_eq!(restored.scheduler.pending, old.scheduler.pending);
    assert_eq!(restored.scheduler.frequency, old.scheduler.frequency);
    assert_eq!(restored.scheduler.ready, 0);
    assert_eq!(restored.scheduler.output, old.output);
    let mut published = false;
    for _ in 0..576 {
        let ready = restored.scheduler.ready;
        let phase = restored.phase;
        let output = restored.output;
        restored.advance(1);
        if restored.output != output {
            assert_eq!(phase % 24, 5);
            for (channel, value) in output.into_iter().enumerate() {
                if restored.output[channel] != value {
                    assert_ne!(ready & (1 << channel), 0);
                }
            }
            published = true;
        }
        let mut writer = Writer::new();
        restored.write_state(&mut writer);
        let mut decoded = Opn2::read_state(&mut Reader::new(&writer.into_bytes())).unwrap();
        assert_eq!(decoded, restored);
        let mut expected = restored;
        for clocks in [0, 5, 1, 6, 7, 143, 144] {
            decoded.advance(clocks);
            expected.advance(clocks);
            assert_eq!(decoded, expected);
        }
    }
    assert!(published);
    assert!(
        restored
            .channels
            .iter()
            .all(|channel| channel.render.mod_valid == 15 && channel.render.raw_valid == 15)
    );
}

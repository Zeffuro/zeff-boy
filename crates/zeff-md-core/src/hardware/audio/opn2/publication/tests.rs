use super::*;

#[test]
fn each_latch_publishes_only_its_channel_and_current_pan() {
    let mut chip = Opn2::default();
    chip.scheduler.output = [-8192, -3200, 0, 32, 3200, 8160];
    let mut expected = [0; CHANNELS];
    for cycle in 0..24 {
        let before = chip.output;
        if cycle % 4 == 0 {
            let channel = [1, 5, 3, 0, 4, 2][cycle / 4];
            chip.channels[channel].write_channel(0xb4 + (channel % 3) as u8, PAN_RIGHT);
            expected[channel] = chip.scheduler.output[channel];
            assert_ne!(chip.output_pan[channel], PAN_RIGHT);
        }
        chip.publish_slot(cycle);
        assert_eq!(chip.output, expected);
        if cycle % 4 != 0 {
            assert_eq!(chip.output, before);
        }
    }
    assert_eq!(chip.output_pan, [PAN_RIGHT; CHANNELS]);
}

#[test]
fn every_fraction_exposes_the_next_latch_deadline() {
    let mut chip = Opn2::default();
    for phase in 0..144 {
        chip.phase = phase;
        let edge = [6, 30, 54, 78, 102, 126, 150]
            .into_iter()
            .find(|edge| *edge > phase)
            .unwrap();
        assert_eq!(chip.clocks_to_sample(), edge - phase);
    }
}

#[test]
fn held_pan_and_ready_channels_are_independent_of_live_writes_and_other_readiness() {
    let mut chip = Opn2 {
        output: [320; CHANNELS],
        output_pan: [PAN_LEFT; CHANNELS],
        ..Opn2::default()
    };
    chip.scheduler.ready = 1 << 1;
    chip.scheduler.output[1] = 3200;
    chip.channels[1].write_channel(0xb5, PAN_RIGHT);
    assert_eq!(
        chip.sample_with_mutes([true, false, true, true, true, true]),
        [320, 0]
    );
    chip.publish_slot(0);
    assert_eq!(
        chip.sample_with_mutes([true, false, true, true, true, true]),
        [0, 3200]
    );
    chip.channels[1].write_channel(0xb5, PAN_LEFT);
    chip.publish_slot(4);
    assert_eq!(chip.output, [320, 3200, 320, 320, 320, 320]);
    assert_eq!(chip.output_pan[1], PAN_RIGHT);
}

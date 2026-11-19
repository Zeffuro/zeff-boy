use crate::hardware::audio::opn2::Opn2;

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

use super::*;

fn mapping() {
    assert_eq!(size_of_val(&SHIFTED_OPERATORS), 48);
    for (cycle, &row) in SHIFTED_OPERATORS.iter().enumerate() {
        assert_eq!(row >> 10, 0);
        let channel = cycle % CHANNELS;
        let next = if channel == 5 { 0 } else { channel + 1 };
        let channels = [
            next,
            if channel >= 4 {
                channel - 4
            } else {
                channel + 2
            },
            if channel < 2 {
                channel + 4
            } else {
                channel - 2
            },
            if channel == 0 { 5 } else { channel - 1 },
            next,
        ];
        for (field, offset) in [SLOTS - 5, SLOTS - 4, SLOTS - 2, SLOTS - 1, 1]
            .into_iter()
            .enumerate()
        {
            let slot = (cycle + offset) % SLOTS;
            assert_eq!(
                channels[field],
                slot % CHANNELS,
                "cycle {cycle} field {field}"
            );
            assert_eq!(
                usize::from((row >> (field * 2)) & 3),
                OPERATOR_ORDER[slot / CHANNELS],
                "cycle {cycle} field {field}"
            );
        }
    }
}

fn tagged(mut chip: Opn2, partial: bool) -> Opn2 {
    for (channel, state) in chip.channels.iter_mut().enumerate() {
        state.set_frequency(0x101 + channel as u16 * 0x103, channel as u8);
        for logical in 0..constants::OPERATORS {
            let tag = (channel * constants::OPERATORS + logical) as u16;
            let pipeline = &mut state.operator_mut(logical).pipeline;
            pipeline.desired = tag & 1 != 0;
            pipeline.csm = channel == 2 && logical & 1 != 0;
            pipeline.sampled = tag & 2 != 0 || pipeline.csm;
            pipeline.increment = 101 + u32::from(tag);
            pipeline.increment_valid = !partial || tag & 1 != 0;
            pipeline.selected.valid = !partial || tag & 2 != 0;
            pipeline.prepared.valid = !partial || tag & 4 != 0;
            pipeline.level_valid = !partial || tag & 8 != 0;
            pipeline.level = 201 + tag * 7;
            pipeline.sample.phase = u32::from(tag + 1) * 1024;
            pipeline.sample.level = 301 + tag * 5;
            pipeline.sample.valid = !partial || tag & 1 == 0;
        }
    }
    chip.special = [(0x123, 1), (0x456, 3), (0x789, 5)];
    chip.scheduler.frequency = (0x367, 4);
    chip.scheduler.frequency_valid = !partial;
    chip
}

pub(super) fn small(report: &mut Report) {
    mapping();
    let base = configured(report, 7, 6, 0x80, 13);
    let before = report.cases;
    for cycle in 0..SLOTS {
        for fraction in 0..SLOT_CLOCKS {
            for mode in [0, 0x40, 0x80, 0xc0] {
                for partial in [false, true] {
                    let mut chip = tagged(base, partial);
                    chip.mode = mode | 15;
                    chip.scheduler.step = 144 + cycle as u32;
                    chip.phase = cycle as u64 * SLOT_CLOCKS + fraction;
                    chip.scheduler.command = Command {
                        channel: cycle as u8 % CHANNELS as u8,
                        mask: (cycle as u8 + 1) & 15,
                    };
                    chip.scheduler.pending = Some(Command {
                        channel: 2,
                        mask: 5,
                    });
                    let mut pair = Pair::new(chip, report);
                    pair.codec();
                    pair.advance(SLOT_CLOCKS - fraction);
                    pair.write(0, 0x28, 0xa2);
                    pair.write(0, 0x27, (mode ^ 0x80) | 15);
                    pair.slots(SLOTS as u64 + 3);
                    pair.write(0, 0x28, 0x52);
                    pair.write(0, 0x27, 0x81);
                    pair.slots(SLOTS as u64 + 3);
                }
            }
        }
    }
    assert_eq!(report.cases - before, 24 * 6 * 4 * 2);
}

#[test]
fn shifted_geometry_all_mappings_and_tagged_partial_slots() {
    let mut report = Report::default();
    small(&mut report);
    assert_ne!(report.finish(), 0);
}

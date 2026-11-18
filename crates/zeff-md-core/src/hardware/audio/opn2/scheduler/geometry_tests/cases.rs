use super::*;

pub(super) fn alignment(report: &mut Report) {
    let base = configured(report, 5, 6, 0x80, 9);
    let before = report.cases;
    for cycle in 0..SLOTS {
        for fraction in 0..SLOT_CLOCKS {
            for channel in 0..CHANNELS {
                for logical in 0..constants::OPERATORS {
                    let mut pair = Pair::new(base, report);
                    pair.bulk(cycle as u64 * SLOT_CLOCKS + fraction);
                    let command = encoded_channel(channel) | (1 << (4 + logical));
                    pair.write(0, 0x28, command);
                    pair.slots(SLOTS as u64 + 7);
                    let physical = OPERATOR_ORDER.iter().position(|&op| op == logical).unwrap();
                    let (bank, address) = voice_register(channel, physical, 0x50);
                    pair.write(bank, address, 0xdf);
                    pair.slots(SLOTS as u64 + 5);
                    pair.write(0, 0x28, encoded_channel(channel));
                    pair.slots(SLOTS as u64 + 3);
                }
            }
        }
    }
    assert_eq!(report.cases - before, 24 * 6 * 6 * 4);
}

pub(super) fn algorithms(report: &mut Report) {
    let before = report.cases;
    for algorithm in 0..8 {
        for feedback in 0..8 {
            for mode in [0, 0x40, 0x80, 0xc0] {
                for flavor in 0..16 {
                    let base = configured(report, algorithm, feedback, mode, flavor);
                    let mut pair = Pair::new(base, report);
                    pair.slots(4 * SLOTS as u64);
                    pair.write(0, 0x27, mode | 0x3f);
                    pair.slots(2 * SLOTS as u64);
                    pair.write(0, 0x27, (mode.wrapping_add(0x40) & 0xc0) | 15);
                    pair.slots(2 * SLOTS as u64);
                }
            }
        }
    }
    assert_eq!(report.cases - before, 8 * 8 * 4 * 16 * 2);
}

pub(super) fn boundaries(report: &mut Report) {
    let base = configured(report, 7, 7, 0x80, 15);
    for step in (0..=CLOCK_PREFIX).chain(CLOCK_END - 2 * SLOTS as u32..CLOCK_END) {
        for fraction in 0..SLOT_CLOCKS {
            let mut chip = base;
            chip.scheduler.step = step;
            chip.phase = u64::from(step % SLOTS as u32) * SLOT_CLOCKS + fraction;
            let mut pair = Pair::new(chip, report);
            pair.codec();
            pair.write(0, 0x28, 0xf2);
            pair.slots(3 * SLOTS as u64);
        }
    }
    for mode in [0, 0x40, 0x80, 0xc0] {
        for reload in [0, 1, 0x3fe, 0x3ff] {
            let mut pair = Pair::new(base, report);
            pair.write(0, 0x27, mode | 0x30);
            pair.write(0, 0x24, (reload >> 2) as u8);
            pair.write(0, 0x25, (reload & 3) as u8);
            pair.write(0, 0x26, (reload >> 2) as u8);
            pair.write(0, 0x27, mode | 15);
            pair.slots(33 * SLOTS as u64);
            pair.write(0, 0x27, mode | 0x3f);
            pair.slots(2 * SLOTS as u64);
            pair.write(0, 0x27, mode);
            pair.slots(2 * SLOTS as u64);
        }
    }
}

pub(super) fn commands(report: &mut Report) {
    let base = configured(report, 3, 7, 0xc0, 8);
    for cycle in 0..SLOTS {
        let mut pair = Pair::new(base, report);
        pair.slots(cycle as u64);
        for mask in 0..16 {
            for encoded in 0..8 {
                pair.write(0, 0x28, mask << 4 | encoded);
                pair.write(0, 0x28, 0xf3);
                pair.write(0, 0x28, mask << 4 | encoded);
                pair.slots(2 * SLOTS as u64);
            }
        }
        pair.write(2, 0x28, 0xff);
        pair.slots(SLOTS as u64);
    }
}

pub(super) fn quiet_and_chunks(report: &mut Report) {
    let mut quiet = Pair::new(Opn2::default(), report);
    quiet.bulk(SLOT_CLOCKS * SLOTS as u64 * 128);
    assert_eq!(quiet.actual.output, [0; CHANNELS]);
    quiet.write(0, 0x22, 15);
    quiet.bulk(SLOT_CLOCKS * SLOTS as u64 * 128 + 5);
    let base = quiet.actual;
    let mut active = Pair::new(base, report);
    active.write(0, 0x30, 1);
    active.write(0, 0x50, 31);
    active.write(0, 0x80, 0xf0);
    active.write(0, 0xa4, 0x2f);
    active.write(0, 0xa0, 0xff);
    active.write(0, 0xb0, 7);
    active.write(0, 0x28, 0x10);
    active.slots(24 * SLOTS as u64);
    assert_ne!(active.actual.output[0], 0);
    active.write(0, 0x28, 0);
    active.bulk(20017);
    let base = configured(report, 4, 5, 0x80, 7);
    for fraction in 0..SLOT_CLOCKS {
        let mut pair = Pair::new(base, report);
        pair.advance(fraction);
        for clocks in [0, 1, 5, 6, 7, 143, 144, 145, 2001, 4097] {
            let mut whole = pair.actual;
            whole.advance(clocks);
            for _ in 0..clocks {
                pair.advance(1);
            }
            assert_eq!(whole, pair.actual);
            pair.write(0, 0x22, 8 | clocks as u8 & 7);
        }
    }
}

pub(super) fn codecs(report: &mut Report) {
    {
        for cycle in 0..SLOTS {
            for fraction in 0..SLOT_CLOCKS {
                let mode = [0, 0x40, 0x80, 0xc0][cycle & 3];
                let base = configured(report, (cycle & 7) as u8, (fraction & 7) as u8, mode, 11);
                let mut pair = Pair::new(base, report);
                pair.bulk(cycle as u64 * SLOT_CLOCKS + fraction);
                for channel in 0..CHANNELS {
                    let (bank, local) = voice_register(channel, 0, 0);
                    pair.write(bank, 0xb4 + local, 0xc0);
                }
                pair.write(0, 0x28, 0xf2);
                pair.codec();
                {}
                {}
                pair.slots(3 * SLOTS as u64);
                pair.codec();
                pair.bulk(1007);
            }
        }
    }
}

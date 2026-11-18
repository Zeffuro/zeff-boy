use super::*;

fn seeded(step: u32) -> Scheduler {
    Scheduler {
        step,
        command: Command {
            channel: 2,
            mask: 5,
        },
        pending: Some(Command {
            channel: 255,
            mask: 10,
        }),
        frequency: (0x567, 6),
        frequency_valid: false,
        output: [-1024, 2048, -3072, 4096, -5120, 6144],
        ready: 0x25,
        accumulator: [-1, 2, -3, 4, -5, 6],
        carriers: [0, 1, 2, 4, 8, 15],
    }
}

#[test]
fn single_slot_clock_matches_bulk_for_every_admitted_step() {
    for step in 0..CLOCK_END {
        let mut actual = seeded(step);
        let mut original = actual;
        actual.clock();
        original.advance_clock(1);
        assert_eq!(actual, original, "step {step}");
    }
}

#[test]
fn single_slot_clock_retains_prefix_and_repeated_wraps() {
    let mut actual = seeded(0);
    for elapsed in 1..=u64::from(CLOCK_PREFIX + 3 * CLOCK_PERIOD) {
        actual.clock();
        let mut original = seeded(0);
        original.advance_clock(elapsed);
        assert_eq!(actual, original, "elapsed {elapsed}");
    }
}

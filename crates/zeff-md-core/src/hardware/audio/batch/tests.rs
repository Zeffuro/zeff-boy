use super::*;

fn active(region: Region, rate: u32) -> Audio {
    let mut audio = Audio::new(region);
    audio.set_sample_rate(rate);
    for value in [0x85, 0x06, 0x90] {
        audio.write_psg(0, value);
    }
    for (port, value) in [(0, 0x2a), (1, 0xd3), (0, 0x2b), (1, 0x80)] {
        audio.write_opn2(0, port, value);
    }
    audio
}

#[test]
fn batched_boundaries_preserve_full_chip_filter_and_queue_state() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [1, 44_100, 48_000, 192_000] {
            let mut reference = active(region, rate);
            let mut actual = reference.clone();
            for end in [1009, 20_003, 55_999, 80_003] {
                actual.begin_batch();
                let mut at = reference.now;
                while at < end {
                    at = (at + 17).min(end);
                    reference.advance_to(at);
                    actual.request_advance_to(at);
                }
                actual.finish_batch();
                assert!(actual == reference, "{region:?} {rate} {end}");
            }
        }
    }
}

#[test]
fn writes_reset_and_status_keep_the_admitted_prefix_and_same_tick_ties() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut reference = active(region, 48_000);
        let mut actual = reference.clone();
        actual.begin_batch();
        for at in [41, 42, 43, 211, 1007, 1008, 1009] {
            reference.advance_to(at);
            actual.request_advance_to(at);
            for audio in [&mut actual, &mut reference] {
                audio.write_opn2(at, 0, 0x2a);
                audio.write_opn2(at, 1, at as u8);
                audio.write_psg(at, 0x90 | (at % 8) as u8);
            }
            actual.flush_pending();
            assert_eq!(
                actual.opn2_status_at(at + 37),
                reference.opn2_status_at(at + 37)
            );
        }
        actual.request_advance_to(1100);
        reference.advance_to(1100);
        actual.flush_pending();
        assert_eq!(actual.now, 1100);
        assert_eq!(actual.opn2_status_at(2000), reference.opn2_status_at(2000));
        assert_eq!(actual.now, 1100);
        for audio in [&mut actual, &mut reference] {
            audio.reset_opn2(1100);
        }
        actual.finish_batch();
        assert!(actual == reference);
    }
}

#[test]
fn disabled_generation_keeps_each_restart_and_long_gaps_keep_original_partition() {
    for enabled in [false, true] {
        for rate in [1, 48_000, 192_000] {
            let mut reference = active(Region::Pal, rate);
            reference.set_sample_generation_enabled(enabled);
            let mut actual = reference.clone();
            actual.begin_batch();
            for at in [
                17,
                17,
                1000,
                MAX_DEFERRED_TICKS - 1,
                MAX_DEFERRED_TICKS + 1,
                20_000_000,
            ] {
                reference.advance_to(at);
                actual.request_advance_to(at);
            }
            actual.finish_batch();
            assert!(actual == reference, "{enabled} {rate}");
            let mut samples = Vec::new();
            actual.drain_into(&mut samples);
            let mut expected = Vec::new();
            reference.drain_into(&mut expected);
            assert_eq!(samples, expected);
            if !enabled {
                assert!(samples.is_empty());
            }
        }
    }
}

#[test]
fn disabled_same_tick_writes_keep_filter_transitions_until_an_explicit_request() {
    for repeat in [false, true] {
        let mut reference = active(Region::NtscUs, 48_000);
        reference.set_sample_generation_enabled(false);
        let mut actual = reference.clone();
        actual.begin_batch();
        reference.advance_to(41);
        actual.request_advance_to(41);
        for audio in [&mut actual, &mut reference] {
            audio.write_psg(41, 0x9f);
            audio.write_opn2(41, 0, 0x2a);
            audio.write_opn2(41, 1, 0x30);
        }
        actual.flush_pending();
        if repeat {
            reference.advance_to(41);
            actual.request_advance_to(41);
        }
        actual.finish_batch();
        assert!(actual == reference, "explicit zero request={repeat}");
    }
}

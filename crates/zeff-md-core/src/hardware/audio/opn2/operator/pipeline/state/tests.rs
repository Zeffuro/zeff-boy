use super::*;

fn bytes(pipeline: Pipeline) -> Vec<u8> {
    let mut writer = Writer::new();
    pipeline.write_state(&mut writer);
    writer.into_bytes()
}

#[test]
fn pending_operator_stages_round_trip_without_reconstruction() {
    let pipeline = Pipeline {
        desired: true,
        sampled: false,
        csm: false,
        selected: Rate {
            enabled: true,
            effective: 63,
            sustain: 992,
            valid: true,
        },
        prepared: Action {
            increment: 8,
            maximum: true,
            sustain: 448,
            valid: false,
        },
        increment: PHASE_MASK,
        increment_valid: false,
        reset: true,
        level_add: 1142,
        total_level: 127,
        level: ENVELOPE_MAX,
        level_valid: false,
        sample: Sample {
            phase: PHASE_MASK - 1,
            level: 17,
            valid: true,
        },
    };
    let saved = bytes(pipeline);
    let mut reader = Reader::new(&saved);
    assert_eq!(Pipeline::read_state(&mut reader).unwrap(), pipeline);
    assert!(reader.is_exhausted());
    for length in 0..saved.len() {
        assert!(Pipeline::read_state(&mut Reader::new(&saved[..length])).is_err());
    }
}

#[test]
fn invalid_pending_operator_values_reject_even_when_stage_is_invalid() {
    let saved = bytes(Pipeline::migrated(false));
    for (offset, replacement) in [
        (3, vec![64]),
        (4, 31_u16.to_le_bytes().to_vec()),
        (7, vec![3]),
        (9, 960_u16.to_le_bytes().to_vec()),
        (12, (PHASE_MASK + 1).to_le_bytes().to_vec()),
        (18, 1143_u16.to_le_bytes().to_vec()),
        (20, (ENVELOPE_MAX + 1).to_le_bytes().to_vec()),
        (23, (PHASE_MASK + 1).to_le_bytes().to_vec()),
        (27, (ENVELOPE_MAX + 1).to_le_bytes().to_vec()),
        (30, vec![129]),
        (30, vec![255]),
    ] {
        let mut bad = saved.clone();
        bad[offset..offset + replacement.len()].copy_from_slice(&replacement);
        assert!(
            Pipeline::read_state(&mut Reader::new(&bad)).is_err(),
            "offset {offset}"
        );
    }
    for offset in [0, 1, 2, 6, 8, 11, 16, 17, 22, 29, 31] {
        let mut bad = saved.clone();
        bad[offset] = 2;
        assert!(
            Pipeline::read_state(&mut Reader::new(&bad)).is_err(),
            "offset {offset}"
        );
    }
}

#[test]
fn csm_samples_require_a_selected_key_and_known_total_level() {
    for (sampled, total_level, valid) in [(false, 0, false), (true, 128, false), (true, 0, true)] {
        let saved = bytes(Pipeline {
            sampled,
            total_level,
            csm: true,
            ..Pipeline::default()
        });
        assert_eq!(
            Pipeline::read_state(&mut Reader::new(&saved)).is_ok(),
            valid
        );
    }
}

#[test]
fn selected_total_level_validates_the_retained_am_split() {
    for (level_add, total_level, valid) in [
        (0, 1, false),
        (503, 63, false),
        (630, 63, true),
        (631, 63, false),
        (1142, 127, true),
        (1142, UNKNOWN_TOTAL_LEVEL, true),
    ] {
        let saved = bytes(Pipeline {
            level_add,
            total_level,
            ..Pipeline::default()
        });
        assert_eq!(
            Pipeline::read_state(&mut Reader::new(&saved)).is_ok(),
            valid
        );
    }
}

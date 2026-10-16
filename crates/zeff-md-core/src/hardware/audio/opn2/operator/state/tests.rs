use super::*;

#[test]
fn invalid_operator_phases_and_envelopes_reject() {
    let original = Operator::default();
    let mut writer = Writer::new();
    original.write_state(&mut writer);
    let saved = writer.into_bytes();
    for (offset, replacement) in [
        (7, (PHASE_MASK + 1).to_le_bytes().to_vec()),
        (11, (ENVELOPE_MAX + 1).to_le_bytes().to_vec()),
        (13, vec![5]),
        (14, vec![2]),
    ] {
        let mut bad = saved.clone();
        bad[offset..offset + replacement.len()].copy_from_slice(&replacement);
        assert!(Operator::read_state(&mut Reader::new(&bad)).is_err());
    }
}

#[test]
fn legacy_operator_accumulators_are_validated_before_migration() {
    let mut writer = Writer::new();
    let mut original = Operator::default();
    original.write(5, 24);
    original.key(true, (0x400, 3));
    original.clock_envelope((0x400, 3), 4);
    original.write_state_version(&mut writer, 1);
    let mut saved = writer.into_bytes();
    saved[14..19].copy_from_slice(&[0xff, 0x3f, 0, 0, 2]);
    let mut expected = original;
    expected.pipeline = Pipeline::migrated(original.keyed);
    for version in 1..=3 {
        let restored = Operator::read_state_version(&mut Reader::new(&saved), version).unwrap();
        assert_eq!(restored, expected);
        for (offset, replacement) in [(14, (1_u32 << 14).to_le_bytes().to_vec()), (18, vec![3])] {
            let mut bad = saved.clone();
            bad[offset..offset + replacement.len()].copy_from_slice(&replacement);
            assert!(Operator::read_state_version(&mut Reader::new(&bad), version).is_err());
        }
    }
}

#[test]
fn old_operator_layouts_seed_ssg_without_reconstructing_missing_history() {
    let mut original = Operator {
        attenuation: 512,
        envelope: Envelope::Sustain,
        keyed: true,
        ..Operator::default()
    };
    original.write(9, 11);
    original.pipeline.sampled = true;
    original.select_rate((0x400, 3), 0, true);
    assert_ne!(original.ssg, ssg::Ssg::default());
    for version in 6..=9 {
        let mut writer = Writer::new();
        original.write_state_version(&mut writer, version);
        let bytes = writer.into_bytes();
        let mut reader = Reader::new(&bytes);
        let loaded = Operator::read_state_version(&mut reader, version).unwrap();
        assert!(reader.is_exhausted());
        let mut expected = Operator {
            ssg: ssg::Ssg::default(),
            ..original
        };
        expected.pipeline.total_level = pipeline::UNKNOWN_TOTAL_LEVEL;
        assert_eq!(loaded, expected);
    }
    original.write(9, 7);
    let mut writer = Writer::new();
    original.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut loaded = Operator::read_state(&mut Reader::new(&bytes)).unwrap();
    assert_eq!(loaded, original);
    assert!(loaded.ssg.enabled);
    loaded.select_rate((0x400, 3), 0, true);
    assert_eq!(loaded.ssg, ssg::Ssg::default());
}

#[test]
fn old_total_level_history_stays_unknown_until_natural_selection() {
    let mut original = Operator {
        attenuation: 211,
        envelope: Envelope::Sustain,
        keyed: true,
        ..Operator::default()
    };
    original.write(4, 63);
    original.write(6, 128);
    original.pipeline.sampled = true;
    original.select_rate((0x400, 3), 37, true);
    original.write(4, 1);
    for version in 1..=10 {
        let mut writer = Writer::new();
        original.write_state_version(&mut writer, version);
        let bytes = writer.into_bytes();
        let mut loaded = Operator::read_state_version(&mut Reader::new(&bytes), version).unwrap();
        assert_eq!(loaded.pipeline.total_level, pipeline::UNKNOWN_TOTAL_LEVEL);
        let pending_add = loaded.pipeline.level_add;
        assert_eq!(
            pending_add,
            if version >= crate::save_state::STATE_VERSION_WITH_SLOT_SCHEDULER {
                504 + 37
            } else {
                0
            }
        );
        let mut writer = Writer::new();
        loaded.write_state(&mut writer);
        let current = writer.into_bytes();
        let mut again = Operator::read_state(&mut Reader::new(&current)).unwrap();
        assert_eq!(again, loaded);
        loaded.generate_level(true);
        assert_eq!(loaded.pipeline.level, (211 + pending_add).min(ENVELOPE_MAX));
        again.generate_level(true);
        assert_eq!(again, loaded);
        loaded.select_rate((0x400, 3), 37, true);
        assert_eq!(loaded.pipeline.total_level, 1);
        assert_eq!(loaded.pipeline.level_add, 8 + 37);
        loaded.generate_level(true);
        assert_eq!(loaded.pipeline.level, 211 + 37);
    }
}

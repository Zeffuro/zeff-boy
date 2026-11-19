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

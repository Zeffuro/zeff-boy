use super::*;

#[test]
fn malformed_psg_indices_modes_and_zero_counters_reject() {
    let mut writer = Writer::new();
    Psg::default().write_state(&mut writer);
    let saved = writer.into_bytes();
    for (offset, replacement) in [
        (0, 0x400_u16.to_le_bytes().to_vec()),
        (6, vec![16]),
        (10, vec![0, 0]),
        (16, vec![2]),
        (19, vec![8]),
        (20, vec![8]),
        (21, vec![0, 0]),
        (24, vec![0, 0]),
    ] {
        let mut bad = saved.clone();
        bad[offset..offset + replacement.len()].copy_from_slice(&replacement);
        assert!(Psg::read_state(&mut Reader::new(&bad)).is_err());
    }
}

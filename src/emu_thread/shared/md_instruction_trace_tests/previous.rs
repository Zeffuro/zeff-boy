use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_previous_ram_values_reach_real_trace_and_first_watch_hit_with_each_lane() {
    let program = [
        0x23fc, 0x192a, 0x3b4c, 0xff, 0x40, 0x207c, 0xff, 0x44, 0x203c, 0xa1b2, 0xc3d4, 0x13fc,
        0x91, 0xe0, 0x40, 0x13fc, 0x82, 0xff, 0x41, 0x33fc, 0x7364, 0xfe, 0x40, 0x33fc, 0x7364,
        0xff, 0x40, 0x23fc, 0x1234, 0x5678, 0xff, 0x40, 0x2100, 0x60fe, 0x4e71,
    ];
    let byte = TraceWriteWidth::Byte;
    let word = TraceWriteWidth::Halfword;
    let writes: &[&[(u32, u32, u32, TraceWriteWidth)]] = &[
        &[(0xe00040, 0x19, 0x91, byte)],
        &[(0xff0041, 0x2a, 0x82, byte)],
        &[(0xfe0040, 0x9182, 0x7364, word)],
        &[(0xff0040, 0x7364, 0x7364, word)],
        &[
            (0xff0040, 0x7364, 0x1234, word),
            (0xff0042, 0x3b4c, 0x5678, word),
        ],
        &[
            (0xff0042, 0x5678, 0xc3d4, word),
            (0xff0040, 0x1234, 0xa1b2, word),
        ],
    ];
    let hits = [
        (0xe00040, 0x19, 0x91),
        (0xff0041, 0x2a, 0x82),
        (0xfe0040, 0x91, 0x73),
        (0xff0040, 0x73, 0x73),
        (0xff0040, 0x73, 0x12),
        (0xff0042, 0x56, 0xc3),
    ];
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture_with_timing(header, &program, timing);
            for _ in 0..3 {
                machine_mut(&mut backend).step_instruction().unwrap();
            }
            backend.debug_suspend();
            let mut expected = machine(&backend).clone();
            for (index, (wanted, &(address, old, new))) in writes.iter().zip(&hits).enumerate() {
                expected.step_instruction().unwrap();
                let mut actions = enabled();
                actions.add_watchpoint = Some((0xe00000, 0xffffff, WatchType::Write));
                let result = run(&mut backend, actions, true, 0, None);
                compare(&backend, &mut expected, &result, 0);
                assert_eq!(
                    machine(&backend).encode_state().unwrap(),
                    expected.encode_state().unwrap()
                );
                assert_eq!(
                    machine(&backend).encode_frontend_state().unwrap(),
                    expected.encode_frontend_state().unwrap()
                );
                let row = entries(&result).last().unwrap();
                assert_eq!(entries(&result).len(), index + 1);
                assert_eq!(row.write_overflow, 0);
                assert_eq!(
                    row.writes()
                        .iter()
                        .map(|write| (write.address, write.old_value, write.new_value, write.width))
                        .collect::<Vec<_>>(),
                    wanted
                        .iter()
                        .map(|&(address, old, new, width)| (address, Some(old), new, width))
                        .collect::<Vec<_>>()
                );
                let hit = result
                    .ui_data
                    .cpu_debug
                    .as_ref()
                    .unwrap()
                    .hit_watchpoint
                    .as_ref()
                    .unwrap();
                assert_eq!(
                    (hit.address, hit.old_value, hit.new_value, hit.watch_type),
                    (address, Some(old), Some(new), WatchType::Write)
                );
            }
        }
    }
}

use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_instruction_trace_movem_trace_keeps_all_writes_and_architectural_register_ids() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture_with_timing(
                header,
                &[0x46fc, 0xa000, 0x48f9, 0xffff, 0xff, 0x40, 0x60fe],
                timing,
            );
            machine_mut(&mut backend).step_instruction().unwrap();
            backend.debug_suspend();
            let before = machine(&backend).registers();
            let mut expected = machine(&backend).clone();
            expected.step_instruction().unwrap();
            let result = run(&mut backend, enabled(), true, 0, None);
            compare(&backend, &mut expected, &result, 0);
            let row = &entries(&result)[0];
            assert_eq!(
                row.instruction_bytes(),
                bytes(&[0x48f9, 0xffff, 0xff, 0x40])
            );
            assert_eq!(row.boundary.unwrap().final_exception, Some(9));
            assert_eq!((row.write_len, row.write_overflow), (35, 0));
            let values: Vec<_> = before
                .d
                .into_iter()
                .chain((0..8).map(|register| before.address_register(register).unwrap()))
                .flat_map(|value| [(value >> 16) as u16, value as u16])
                .collect();
            let mut writes: Vec<_> = values
                .into_iter()
                .enumerate()
                .map(|(index, value)| (0xff0040 + index as u32 * 2, u32::from(value)))
                .collect();
            writes.extend([(0xfffffa, 0x20c), (0xfffff6, 0xa000), (0xfffff8, 0)]);
            assert_eq!(
                row.writes()
                    .iter()
                    .map(|write| (write.address, write.new_value))
                    .collect::<Vec<_>>(),
                writes
            );
            assert!(row.writes().iter().all(
                |write| write.old_value == Some(0) && write.width == TraceWriteWidth::Halfword
            ));

            let mut load = fixture_with_timing(
                header,
                &[0x46fc, 0xa000, 0x4cf9, 0xffff, 0, 0x380, 0x60fe],
                timing,
            );
            let words: Vec<_> = (0..16)
                .map(|index| {
                    if index == 15 {
                        0x00fff800
                    } else {
                        0x11110000 + index
                    }
                })
                .flat_map(|value| [(value >> 16) as u16, value as u16])
                .collect();
            let codes_text = words
                .iter()
                .enumerate()
                .map(|(index, word)| format!("{:06X}:{word:04X}", 0x380 + index * 2))
                .collect::<Vec<_>>()
                .join("+");
            assert!(update(&mut load, &mut Vec::new(), codes(&codes_text)).is_none());
            machine_mut(&mut load).step_instruction().unwrap();
            load.debug_suspend();
            let mut expected = machine(&load).clone();
            expected.step_instruction().unwrap();
            let result = run(&mut load, enabled(), true, 0, None);
            compare(&load, &mut expected, &result, 0);
            let row = &entries(&result)[0];
            assert_eq!(
                (row.register_delta_len, row.register_delta_overflow),
                (18, 0)
            );
            let mut ids: std::collections::BTreeSet<_> = row
                .register_deltas()
                .iter()
                .map(|delta| delta.register)
                .collect();
            assert_eq!(ids, (0..19).filter(|&id| id != 15).collect());

            let mut usp =
                fixture_with_timing(header, &[0x207c, 0xff, 0x1000, 0x4e60, 0x60fe], timing);
            machine_mut(&mut usp).step_instruction().unwrap();
            usp.debug_suspend();
            let result = run(&mut usp, enabled(), true, 0, None);
            let row = &entries(&result)[0];
            assert!(
                row.register_deltas()
                    .iter()
                    .any(|delta| delta.register == 15 && delta.value == 0xff1000)
            );
            ids.extend(row.register_deltas().iter().map(|delta| delta.register));
            assert_eq!(ids, (0..19).collect());
        }
    }
}

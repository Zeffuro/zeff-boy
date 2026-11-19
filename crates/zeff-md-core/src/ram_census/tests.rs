use super::*;

fn rom() -> Vec<u8> {
    let program: [u16; 15] = [
        0x13fc, 0x0084, 0x00c0, 0x0011, 0x13fc, 0x0001, 0x00c0, 0x0011, 0x13fc, 0x0090, 0x00c0,
        0x0011, 0x60fe, 0x4e71, 0x4e71,
    ];
    let mut bytes = vec![0; 0x200];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.into_iter().enumerate() {
        bytes[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    bytes
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ram_census_cold_and_retained_whole_machine_pcm_and_closed_owners() {
    let (source, _) = execute(rom(), None, Region::NtscUs, ControllerKind::ThreeButton, 1).unwrap();
    let saved = source.encode_state().unwrap();
    drop(source);
    for restore in [None, Some(saved.as_slice())] {
        let result = run(
            rom(),
            restore,
            Region::NtscUs,
            ControllerKind::ThreeButton,
            2,
        )
        .unwrap();
        assert_eq!(result.fields, 2);
        assert_eq!(result.scope, "DirectedOwnerThreadOnly");
        assert!(!result.full_coverage);
        assert!(result.samples > 3_000 && result.nonzero_samples > 0);
        assert_eq!(
            result.counters[Metric::OwnersCreated as usize],
            result.counters[Metric::OwnersClosed as usize]
        );
        assert!(result.counters[Metric::OwnersCreated as usize] > 0);
        assert_eq!(result.counters[Metric::Failures as usize], 0);
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_test::console_log!("ram-census-synthetic {result:?}");
        #[cfg(not(target_arch = "wasm32"))]
        println!("ram-census-synthetic {result:?}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ram_census_rejects_bounds_bad_restore_and_guest_fault_then_recovers() {
    for fields in [0, 121] {
        assert!(
            run(
                rom(),
                None,
                Region::NtscUs,
                ControllerKind::ThreeButton,
                fields
            )
            .is_err()
        );
    }
    assert!(
        run(
            rom(),
            Some(&[]),
            Region::NtscUs,
            ControllerKind::ThreeButton,
            1
        )
        .is_err()
    );
    let mut faulting = rom();
    faulting[0x100..0x108].copy_from_slice(&[0x10, 0x39, 0, 0xc0, 0, 0x10, 0x4e, 0x71]);
    assert!(
        run(
            faulting.clone(),
            None,
            Region::NtscUs,
            ControllerKind::ThreeButton,
            1
        )
        .is_err()
    );
    let (control, pcm) =
        execute(rom(), None, Region::NtscUs, ControllerKind::ThreeButton, 1).unwrap();
    for failure in 0..4 {
        let mut expected_pcm = pcm.clone();
        if failure == 3 {
            expected_pcm[0] ^= 1;
        }
        let observation = observe(&control, &expected_pcm, || {
            let mut bytes = if failure == 1 {
                faulting.clone()
            } else {
                rom()
            };
            if failure == 2 {
                bytes[0x103] ^= 1;
            }
            execute(
                bytes,
                (failure == 0).then_some(&[][..]),
                Region::NtscUs,
                ControllerKind::ThreeButton,
                1,
            )
        });
        assert!(observation.result.is_err());
        assert!(observation.report.closed(), "{:?}", observation.report);
        assert!(observation.report.count(Metric::OwnersCreated) > 0);
        assert!(observer::idle());
    }
    let outer = Session::begin();
    let before = outer.report();
    assert!(run(rom(), None, Region::NtscUs, ControllerKind::ThreeButton, 1).is_err());
    assert_eq!(outer.report(), before);
    assert!(outer.finish().closed());
    assert!(run(rom(), None, Region::NtscUs, ControllerKind::ThreeButton, 1).is_ok());
}

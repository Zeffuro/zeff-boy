use super::*;

fn collecting_write(
    bus: &mut Bus,
    request: BusAccess,
    value: u16,
    observed: bool,
) -> ([Option<u8>; 2], Witness) {
    let start = bus.cpu_clock;
    let mut expected = bus.clone();
    let normal = expected
        .owned_context()
        .unwrap()
        .write_cycle_observed(request, value, start);
    let mut context = bus.owned_context().unwrap();
    context.work_witness = Some(Witness::default());
    let (cycle, previous) = if observed {
        let (result, previous) = context.write_cycle_observed_with_previous(request, value, start);
        assert_eq!(result.samples, normal.samples);
        assert!(result.samples.is_none());
        (result.cycle, previous)
    } else {
        context.write_cycle_with_previous(request, value, start)
    };
    let witness = context.work_witness.take().unwrap();
    assert_eq!(cycle, normal.cycle);
    cycle.result.unwrap();
    assert_eq!(*bus, expected);
    assert_eq!(raw_bus(bus), raw_bus(&expected));
    witness.check_links();
    (previous, witness)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_previous_api_retains_first_real_pulse_per_lane_and_request() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let base = fixture(region, &[0x60fe, 0x4e71], 0);
        for address in [0xff3ffe, 0xff4000, 0xff7ffe, 0xff8000, 0xe08000] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                for observed in [false, true] {
                    let mut bus = base.clone();
                    let start = bus.cpu_clock;
                    bus.owned_context()
                        .unwrap()
                        .write_cycle(access(address, ByteLanes::Both), 0x1122, start)
                        .result
                        .unwrap();
                    let request = access(address | u32::from(lanes == ByteLanes::Lower), lanes);
                    let (previous, witness) = collecting_write(&mut bus, request, 0x3344, observed);
                    assert_eq!(
                        previous,
                        match lanes {
                            ByteLanes::Upper => [Some(0x11), None],
                            ByteLanes::Lower => [None, Some(0x22)],
                            ByteLanes::Both => [Some(0x11), Some(0x22)],
                        }
                    );
                    witness.check_main_suffix(0, request, 0x3344, 0x1122);
                    let (previous, witness) = collecting_write(&mut bus, request, 0x3344, observed);
                    assert_eq!(
                        previous,
                        match lanes {
                            ByteLanes::Upper => [Some(0x33), None],
                            ByteLanes::Lower => [None, Some(0x44)],
                            ByteLanes::Both => [Some(0x33), Some(0x44)],
                        }
                    );
                    witness.check_main_suffix(0, request, 0x3344, 0x3344);
                    let alias = (address & 0xffff)
                        | if address & 0xff0000 == 0xff0000 {
                            0xe00000
                        } else {
                            0xff0000
                        };
                    let alias = access(alias | u32::from(lanes == ByteLanes::Lower), lanes);
                    let (previous, witness) = collecting_write(&mut bus, alias, 0x5566, observed);
                    assert_eq!(
                        previous,
                        match lanes {
                            ByteLanes::Upper => [Some(0x33), None],
                            ByteLanes::Lower => [None, Some(0x44)],
                            ByteLanes::Both => [Some(0x33), Some(0x44)],
                        }
                    );
                    witness.check_main_suffix(0, alias, 0x5566, 0x3344);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_previous_api_ignores_real_same_address_sound_prefix_even_with_equal_data() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let frontier = sound_write_frontier(region);
        assert_eq!(frontier.ram[0x8000], 0x11);
        for observed in [false, true] {
            for read_first in [false, true] {
                for driven in [0x3300, 0x2200] {
                    let mut bus = frontier.clone();
                    if read_first {
                        let start = bus.cpu_clock;
                        let mut expected = bus.clone();
                        let result = bus
                            .owned_context()
                            .unwrap()
                            .read_cycle(access(0xff8000, ByteLanes::Both), start);
                        assert_eq!(
                            result,
                            expected
                                .owned_context()
                                .unwrap()
                                .read_cycle(access(0xff8000, ByteLanes::Both), start)
                        );
                        assert_eq!(result.result, Ok(0x2200));
                        assert_eq!(bus, expected);
                        assert_eq!(raw_bus(&bus), raw_bus(&expected));
                    }
                    let request = access(0xff8000, ByteLanes::Upper);
                    let (previous, witness) = collecting_write(&mut bus, request, driven, observed);
                    assert_eq!(previous, [Some(0x22), None]);
                    let first_main = if read_first {
                        0
                    } else {
                        witness.check_sound_prefix(request, 0x11)
                    };
                    witness.check_main_suffix(first_main, request, driven, 0x2200);
                    assert_eq!(bus.ram[0x8000], (driven >> 8) as u8);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_previous_api_rejected_offer_clock_fault_rom_and_io_have_no_storage_claim() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let base = fixture(region, &[0x60fe, 0x4e71], 0);
        for observed in [false, true] {
            for (address, mismatch, value, succeeds) in [
                (0xff0040, true, 0x1234, false),
                (0x800000, false, 0x1234, false),
                (0x380, false, 0x1234, true),
                (0xa10008, false, 0, true),
            ] {
                let mut actual = base.clone();
                let mut expected = base.clone();
                let start = CpuClocks(base.cpu_clock.0 + u64::from(mismatch));
                let request = access(address, ByteLanes::Both);
                let normal = expected
                    .owned_context()
                    .unwrap()
                    .write_cycle_observed(request, value, start);
                let (cycle, previous) = if observed {
                    let (result, previous) = actual
                        .owned_context()
                        .unwrap()
                        .write_cycle_observed_with_previous(request, value, start);
                    assert_eq!(result.samples, normal.samples);
                    (result.cycle, previous)
                } else {
                    actual
                        .owned_context()
                        .unwrap()
                        .write_cycle_with_previous(request, value, start)
                };
                assert_eq!(cycle, normal.cycle);
                assert_eq!(cycle.result.is_ok(), succeeds);
                assert_eq!(previous, [None, None]);
                assert_eq!(actual, expected);
            }
        }
    }
}

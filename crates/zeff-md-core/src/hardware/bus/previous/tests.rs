use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn final_error_discards_populated_previous_lanes_without_altering_the_cycle() {
    for result in [Ok(()), Err("tail")] {
        let cycle = CycleResult {
            result,
            wait_clocks: 7,
        };
        let before = cycle.clone();
        let previous = Previous([Some(0x19), Some(0x2a)]).finish(cycle.result.is_ok());
        assert_eq!(
            previous,
            if cycle.result.is_ok() {
                [Some(0x19), Some(0x2a)]
            } else {
                [None, None]
            }
        );
        assert_eq!(cycle, before);
    }
}

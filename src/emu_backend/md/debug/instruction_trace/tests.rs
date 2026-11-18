use super::*;
use zeff_emu_common::debug::WatchType;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn capture_only_requests_previous_values_for_trace_records_or_watchpoints() {
    for (history, source_stop, trace, watch, wanted) in [
        (false, false, false, false, false),
        (true, false, false, false, false),
        (false, true, false, false, false),
        (true, true, false, false, false),
        (false, false, true, false, true),
        (false, false, false, true, true),
        (false, false, true, true, true),
        (true, true, true, true, true),
    ] {
        let mut debugger = PcBreakpoints::default();
        debugger.history.enabled = history;
        if source_stop {
            debugger.rom.offsets.push(0x200);
        }
        if watch {
            debugger
                .controller
                .add_watchpoint(0xff0040, WatchType::Write);
        }
        debugger.trace.set_enabled(trace);
        let record = debugger
            .trace
            .is_enabled()
            .then(|| InstructionTraceRecord::new(TraceExecMode::M68000, 0x200, None, 0, 0, &[]));
        let capture = Capture {
            debugger: &mut debugger,
            record,
            first_input: None,
            outcome: None,
        };
        assert_eq!(
            capture.needs_previous_values(),
            wanted,
            "history={history}, source={source_stop}, trace={trace}, watch={watch}"
        );
    }
}

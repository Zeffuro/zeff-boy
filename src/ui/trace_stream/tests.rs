use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_stream_allocator_fences_exhaustion_without_wrapping_or_global_mutation() {
    let counter = AtomicU64::new(u64::MAX - 1);
    let last = TraceStreamId::allocate_from(&counter);
    let terminal = TraceStreamId::allocate_from(&counter);
    assert!(last.is_live());
    assert!(!terminal.is_live());
    assert!(terminal > last);
    assert_eq!(TraceStreamId::allocate_from(&counter), terminal);
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
    assert_eq!(
        TraceCursor {
            target: crate::ui::TraceTarget::Primary,
            stream: Some(terminal),
            after_sequence: None
        }
        .update(Some(last), Some(500)),
        TraceUpdate::Ignore
    );
}

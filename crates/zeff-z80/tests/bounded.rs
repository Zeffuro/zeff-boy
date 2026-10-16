#[path = "bounded/cases.rs"]
mod cases;
#[test]
fn finite_streams_match_legacy() {
    cases::finite_streams_match_legacy();
}
#[test]
fn long_prefix_stream_is_bounded() {
    cases::long_prefix_stream_is_bounded();
}
#[test]
fn interrupt_and_suspend_deferral() {
    cases::interrupt_and_suspend_deferral();
}
#[test]
fn ignored_index_ed_repeats_at_ed() {
    cases::ignored_index_ed_repeats_at_ed();
}
#[test]
fn io_offsets_are_segment_relative() {
    cases::io_offsets_are_segment_relative();
}

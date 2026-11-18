#[path = "vdp_clock_transition/mod.rs"]
mod vdp_clock_transition;

#[test]
fn actual_guests_publish_width_and_retire_captured_fifo() {
    vdp_clock_transition::actual_guests_publish_width_and_retire_captured_fifo();
}

#[test]
fn raw_and_frontend_restores_preserve_video_audio_and_machine_future() {
    vdp_clock_transition::raw_and_frontend_restores_preserve_video_audio_and_machine_future();
}

#[path = "im0/cases.rs"]
mod cases;

#[test]
fn im0_nop_acceptance() {
    cases::im0_nop_acceptance();
}

#[test]
fn im0_literals_match_all_steppers() {
    cases::im0_literals_match_all_steppers();
}

#[test]
fn im0_instruction_bytes_are_live_and_held() {
    cases::im0_instruction_bytes_are_live_and_held();
}

#[test]
fn im0_pending_clone_suspend_reset() {
    cases::im0_pending_clone_suspend_reset();
}

#[test]
fn im0_completed_codec_layout_and_tags() {
    cases::im0_completed_codec_layout_and_tags();
}

#[test]
fn im0_default_reads_and_remaining_guards() {
    cases::im0_default_reads_and_remaining_guards();
}

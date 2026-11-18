#[path = "vdp_dma_alias/cases.rs"]
mod cases;

#[test]
fn guest_copy_aliases_and_state_continuation() {
    cases::guest_copy_aliases_and_state_continuation();
}

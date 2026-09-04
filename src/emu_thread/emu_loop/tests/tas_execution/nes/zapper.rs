use super::{acquire, request, tas_nes_test_loop};
use crate::emu_thread::{EmuResponse, TasInputFrame};

#[test]
fn direct_nes_run_preserves_zapper_input() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    emu_loop
        .backend
        .set_zapper_state(true, false, false, Some((120, 80)));
    let (lease_id, start_state) = acquire(&mut emu_loop, &responses);
    let input = TasInputFrame {
        zapper: zeff_emu_common::replay::ReplayZapperFrame {
            enabled: true,
            trigger: true,
            hit: false,
            screen_pos: Some((120, 80)),
        },
        ..TasInputFrame::default()
    };

    assert!(emu_loop.handle_command(request(lease_id, 1, start_state, vec![input],)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            executed_project_frames: 1,
            ..
        }
    ));
    assert_eq!(
        emu_loop
            .backend
            .nes_has_standard_or_zapper_controller_topology(),
        Some(true)
    );
    assert_eq!(
        emu_loop.backend.nes_has_standard_controller_topology(),
        Some(false)
    );
}

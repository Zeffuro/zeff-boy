use super::*;
use crate::app::pause::PauseState;

fn effective(coordinator: &TasControlCoordinator, pause: &PauseState) -> bool {
    pause.effective(!coordinator.gameplay_commands_allowed())
}

#[test]
fn user_intent_survives_held_rollback_and_terminal_states() {
    let mut coordinator = TasControlCoordinator::new();
    let mut pause = PauseState::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 4));
    assert!(effective(&coordinator, &pause));

    pause.set_user_paused(true);
    coordinator.cancel();
    consume(
        &mut coordinator,
        EmuResponse::TasControlRollbackRejected {
            requested_lease_id: 4,
            reason: TasControlRollbackRejectedReason::WrongLease { active_lease_id: 5 },
        },
    );
    pause.set_user_paused(false);
    assert!(effective(&coordinator, &pause));
    pause.set_user_paused(true);
    coordinator.retire_worker(WORKER_GENERATION);
    assert!(effective(&coordinator, &pause));
}

#[test]
fn focus_cause_acquired_during_lease_survives_rollback() {
    let mut coordinator = TasControlCoordinator::new();
    let mut pause = PauseState::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 9));
    pause.set_focus(true);
    coordinator.cancel();
    consume(&mut coordinator, rolled_back(9, 73));

    assert_eq!(coordinator.state, TasControlState::Detached);
    assert!(effective(&coordinator, &pause));
    pause.set_focus(false);
    assert!(!effective(&coordinator, &pause));
}

#[test]
fn wrong_generation_retirement_cannot_remove_tas_pause_cause() {
    let mut coordinator = TasControlCoordinator::new();
    let pause = PauseState::new();
    acquire(&mut coordinator);

    coordinator.retire_worker(WORKER_GENERATION + 1);

    assert!(effective(&coordinator, &pause));
    coordinator.retire_worker(WORKER_GENERATION);
    assert!(!effective(&coordinator, &pause));
}

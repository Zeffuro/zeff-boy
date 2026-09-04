use super::*;

fn terminalize(
    coordinator: &mut TasControlCoordinator,
    generation: u64,
    reason: TasControlTerminalReason,
) -> bool {
    coordinator.terminalize_worker(generation, reason)
}

fn assert_terminal(coordinator: &TasControlCoordinator, reason: TasControlTerminalReason) {
    assert_eq!(
        coordinator.state,
        TasControlState::Terminal {
            worker_generation: WORKER_GENERATION,
            reason,
        }
    );
    assert!(!coordinator.gameplay_commands_allowed());
}

#[test]
fn failed_acquisition_send_terminalizes_pending_generation() {
    let mut coordinator = TasControlCoordinator::new();
    acquire(&mut coordinator);

    assert!(terminalize(
        &mut coordinator,
        WORKER_GENERATION,
        TasControlTerminalReason::CommandChannelClosed,
    ));

    assert_terminal(&coordinator, TasControlTerminalReason::CommandChannelClosed);
}

#[test]
fn runtime_fault_discards_held_proof_until_matching_retirement() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 41));

    assert!(terminalize(
        &mut coordinator,
        WORKER_GENERATION,
        TasControlTerminalReason::RuntimeFault,
    ));
    assert_terminal(&coordinator, TasControlTerminalReason::RuntimeFault);

    assert!(coordinator.cancel().is_none());
    assert!(!coordinator.retire_worker(WORKER_GENERATION + 1));
    assert_terminal(&coordinator, TasControlTerminalReason::RuntimeFault);

    assert!(coordinator.retire_worker(WORKER_GENERATION));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn response_loss_terminalizes_rollback_pending_until_retirement() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 8));
    coordinator.cancel().unwrap();

    assert!(terminalize(
        &mut coordinator,
        WORKER_GENERATION,
        TasControlTerminalReason::ResponseChannelClosed,
    ));
    assert_terminal(
        &coordinator,
        TasControlTerminalReason::ResponseChannelClosed,
    );

    coordinator.retire_worker(WORKER_GENERATION);
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn command_loss_terminalizes_commit_pending_until_retirement() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 9));
    complete_execution(&mut coordinator, 9, 1);
    let current = snapshot(0, "main", 0);
    coordinator.commit(Some(&current)).unwrap();

    assert!(terminalize(
        &mut coordinator,
        WORKER_GENERATION,
        TasControlTerminalReason::CommandChannelClosed,
    ));
    assert_terminal(&coordinator, TasControlTerminalReason::CommandChannelClosed);
    assert!(coordinator.retire_worker(WORKER_GENERATION));
    assert_eq!(coordinator.state, TasControlState::Detached);
}

#[test]
fn terminal_state_ignores_late_control_responses() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    terminalize(
        &mut coordinator,
        WORKER_GENERATION,
        TasControlTerminalReason::ResponseChannelClosed,
    );

    for response in [
        acquired(request_id, 7),
        rolled_back(7, 73),
        EmuResponse::TasControlAcquireRejected {
            request_id,
            reason: TasControlAcquireRejectedReason::RuntimeFault,
        },
        EmuResponse::TasControlRollbackRejected {
            requested_lease_id: 7,
            reason: TasControlRollbackRejectedReason::NoActiveLease,
        },
        EmuResponse::TasControlCommitted { lease_id: 7 },
        EmuResponse::TasControlCommitRejected {
            requested_lease_id: 7,
            reason: TasControlCommitRejectedReason::NoActiveLease,
        },
    ] {
        assert!(matches!(
            consume(&mut coordinator, response),
            ResponseDisposition::Consumed { follow_up: None }
        ));
        assert_terminal(
            &coordinator,
            TasControlTerminalReason::ResponseChannelClosed,
        );
    }
}

#[test]
fn wrong_generation_terminal_event_is_inert() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);

    assert!(!terminalize(
        &mut coordinator,
        WORKER_GENERATION + 1,
        TasControlTerminalReason::RuntimeFault,
    ));
    assert!(matches!(
        coordinator.state,
        TasControlState::AcquirePending {
            worker_generation: WORKER_GENERATION,
            request_id: actual,
            ..
        } if actual == request_id
    ));
}

#[test]
fn late_rollback_cannot_clear_a_terminal_fault() {
    let mut coordinator = TasControlCoordinator::new();
    let request_id = acquire(&mut coordinator);
    consume(&mut coordinator, acquired(request_id, 11));
    coordinator.cancel().unwrap();
    terminalize(
        &mut coordinator,
        WORKER_GENERATION,
        TasControlTerminalReason::CommandChannelClosed,
    );

    consume(&mut coordinator, rolled_back(11, 73));

    assert_terminal(&coordinator, TasControlTerminalReason::CommandChannelClosed);
}

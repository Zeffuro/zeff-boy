use super::*;

#[test]
fn linked_editor_stays_editable_and_close_preserves_the_linked_game_position() {
    let mut state = TasEditorWindowState::new();
    assert!(
        !TasEditorLiveStatus::Ready {
            recording_available: true
        }
        .locks_editor()
    );
    assert!(
        TasEditorLiveStatus::Staging {
            completed: 0,
            total: 1,
        }
        .locks_editor()
    );
    assert!(TasEditorLiveStatus::AdvancingFrame.locks_editor());
    assert!(TasEditorLiveStatus::Recording.locks_editor());
    assert!(TasEditorLiveStatus::Terminal("worker lost".to_owned()).locks_editor());

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 1,
        recording_available: true,
    });
    assert!(!state.live_status.locks_editor());
    state.close();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::KeepResultAndReturnToGame
        ))
    );

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 1,
        recording_available: false,
    });
    state.close();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::KeepResultAndReturnToGame
        ))
    );

    let mut initial_advance = TasEditorWindowState::new();
    initial_advance.set_live_status(TasEditorLiveStatus::AdvancingFrame);
    initial_advance.close();
    assert_eq!(
        initial_advance.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::ReturnToGameUnchanged
        ))
    );
}

#[test]
fn close_during_a_linked_command_waits_then_disconnects_with_the_completed_position_once() {
    let mut state = TasEditorWindowState::new();
    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 4,
        recording_available: true,
    });
    state.set_live_status(TasEditorLiveStatus::AdvancingFrame);

    state.close();
    state.close();
    assert!(!state.open);
    assert_eq!(state.take_pending_host_request(), None);

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 5,
        recording_available: true,
    });
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::KeepResultAndReturnToGame
        ))
    );

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 5,
        recording_available: true,
    });
    assert_eq!(state.take_pending_host_request(), None);
}

#[test]
fn close_stops_realtime_recording_before_keeping_the_completed_position() {
    let mut state = TasEditorWindowState::new();
    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 4,
        recording_available: true,
    });
    state.set_live_status(TasEditorLiveStatus::Recording);

    state.close();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::StopRealtimeRecording
        ))
    );

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 5,
        recording_available: true,
    });
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::KeepResultAndReturnToGame
        ))
    );
}

#[test]
fn close_pauses_playback_before_keeping_the_settled_boundary() {
    let mut state = TasEditorWindowState::new();
    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 4,
        recording_available: true,
    });
    state.set_live_status(TasEditorLiveStatus::Playing {
        cursor: 4,
        pause_pending: false,
    });

    state.close();
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::PausePlayback
        ))
    );

    state.set_live_status(TasEditorLiveStatus::Linked {
        cursor: 5,
        recording_available: true,
    });
    assert_eq!(
        state.take_pending_host_request(),
        Some(TasEditorHostRequest::Live(
            TasEditorLiveAction::KeepResultAndReturnToGame
        ))
    );
}

#[test]
fn separate_window_focus_request_is_one_shot_and_clears_when_embedded_or_closed() {
    let mut state = TasEditorWindowState::new();
    state.open_separate_window();
    assert!(state.take_separate_focus_request());
    assert!(!state.take_separate_focus_request());

    state.open_separate_window();
    state.open_embedded();
    assert!(!state.take_separate_focus_request());

    state.open_separate_window();
    state.close();
    assert!(!state.take_separate_focus_request());
}

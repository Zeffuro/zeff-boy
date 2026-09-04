use super::*;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_replay_start_is_committed_only_after_send() {
    let mut pending = None;
    let mut next_capture_id = 8;
    commit_pending_replay_start(
        &mut pending,
        &mut next_capture_id,
        PendingReplayStart {
            path: PathBuf::from("test.zrpl"),
            capture_id: 8,
        },
        9,
        false,
    );
    assert!(pending.is_none());
    assert_eq!(next_capture_id, 8);

    let (capture_id, reserved_next) = replay_capture_id_reservation(next_capture_id).unwrap();
    commit_pending_replay_start(
        &mut pending,
        &mut next_capture_id,
        PendingReplayStart {
            path: PathBuf::from("test.zrpl"),
            capture_id,
        },
        reserved_next,
        true,
    );
    assert_eq!(pending.unwrap().capture_id, 8);
    assert_eq!(next_capture_id, 9);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn replay_capture_id_reservation_fails_closed() {
    assert_eq!(replay_capture_id_reservation(4), Some((4, 5)));
    assert_eq!(replay_capture_id_reservation(u64::MAX), None);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_replay_cancellation_clears_latch_on_channel_loss() {
    let mut pending = Some(PendingReplayStart {
        path: PathBuf::from("test.zrpl"),
        capture_id: 3,
    });
    assert_eq!(
        finish_pending_replay_cancellation(&mut pending, Err(EmuCommandSendError::ChannelClosed)),
        Err(EmuCommandSendError::ChannelClosed)
    );
    assert!(pending.is_none());
}

#[test]
fn checkpoint_bookkeeping_changes_only_after_send() {
    let mut marker = 300;
    commit_checkpoint_marker(&mut marker, 600, false);
    assert_eq!(marker, 300);
    commit_checkpoint_marker(&mut marker, 600, true);
    assert_eq!(marker, 600);

    let mut pending = std::collections::BTreeMap::new();
    commit_pending_checkpoint(&mut pending, 600, [7; 32], false);
    assert!(pending.is_empty());
    commit_pending_checkpoint(&mut pending, 600, [7; 32], true);
    assert_eq!(pending.get(&600), Some(&[7; 32]));
}

#[test]
fn replay_stop_channel_loss_requires_fallback_and_preserves_error() {
    assert_eq!(
        replay_stop_post_capture(Ok(())),
        ReplayStopPostCapture::AwaitFinalState
    );
    assert_eq!(
        replay_stop_post_capture(Err(EmuCommandSendError::ChannelClosed)),
        ReplayStopPostCapture::SaveWithoutFinalState(EmuCommandSendError::ChannelClosed)
    );
}

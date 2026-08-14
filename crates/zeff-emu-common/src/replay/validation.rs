use super::{ReplayJoypadFrame, ReplayMetadata};

pub(super) fn pad_frames_to_metadata_events(
    frames: &mut Vec<ReplayJoypadFrame>,
    metadata: &ReplayMetadata,
) {
    let Some(required_frames) = metadata
        .events
        .iter()
        .filter_map(|event| {
            let frame = usize::try_from(event.frame()).ok()?;
            if event.is_frame_boundary_event() {
                Some(frame)
            } else {
                frame.checked_add(1)
            }
        })
        .max()
    else {
        return;
    };
    if frames.len() >= required_frames {
        return;
    }

    let pad_frame = frames.last().cloned().unwrap_or_default();
    frames.resize(required_frames, pad_frame);
}

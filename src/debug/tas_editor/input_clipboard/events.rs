use anyhow::{Result, bail};
use zeff_emu_common::replay::ReplayEvent;

use super::set_event_frame;
use crate::{
    debug::tas_editor::event_editor::{
        can_author_fds_events, is_editable_event, project_media_id, validate_timeline,
    },
    tas_project::TasEditorSession,
};

pub(super) fn validate_copied_events(
    session: &TasEditorSession,
    events: &[ReplayEvent],
) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let identity = session.project().identity();
    let media_id = project_media_id(identity);
    if !can_author_fds_events(identity)
        || events
            .iter()
            .any(|event| !is_editable_event(event, &media_id))
    {
        bail!("only editable FDS drive events can be copied with input frames");
    }
    Ok(())
}

pub(super) fn replacement_events(
    session: &TasEditorSession,
    branch_id: &str,
    start: u64,
    end: u64,
    relative_events: &[ReplayEvent],
    include_events: bool,
) -> Result<Option<Vec<ReplayEvent>>> {
    if !include_events {
        return Ok(None);
    }
    let branch = session
        .project()
        .branch(branch_id)
        .ok_or_else(|| anyhow::anyhow!("paste target branch no longer exists"))?;
    let replaced = branch
        .events()
        .iter()
        .filter(|event| event.frame() >= start && event.frame() < end)
        .cloned()
        .collect::<Vec<_>>();
    validate_copied_events(session, &replaced)?;
    validate_copied_events(session, relative_events)?;
    let mut events = branch
        .events()
        .iter()
        .filter(|event| event.frame() < start || event.frame() >= end)
        .cloned()
        .collect::<Vec<_>>();
    for event in relative_events {
        let mut event = event.clone();
        let frame = start
            .checked_add(event.frame())
            .ok_or_else(|| anyhow::anyhow!("pasted TAS event frame overflows"))?;
        set_event_frame(&mut event, frame);
        events.push(event);
    }
    events.sort_by(ReplayEvent::canonical_cmp);
    let media_id = project_media_id(session.project().identity());
    validate_timeline(&events, branch.frame_count(), &media_id)?;
    Ok(Some(events))
}

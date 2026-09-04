use super::action::TasTimelineNavigation;
use crate::tas_project::TasEditorSession;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TasInputSelection {
    pub(super) branch_id: String,
    pub(super) start: u64,
    pub(super) end: u64,
}

impl TasInputSelection {
    pub(super) fn length(&self) -> u64 {
        self.end - self.start
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TasTimelineSelection {
    End {
        branch_id: String,
    },
    Rows {
        branch_id: String,
        anchor: u64,
        active: u64,
    },
}

#[derive(Clone)]
pub(super) struct TasTimelineSelectionState {
    selection: Option<TasTimelineSelection>,
}

impl TasTimelineSelectionState {
    pub(super) fn new() -> Self {
        Self { selection: None }
    }

    pub(super) fn reset(&mut self) {
        self.selection = None;
    }

    pub(super) fn select_frame(&mut self, session: &TasEditorSession, frame: u64, extend: bool) {
        self.sync(session);
        let Some(last) = session.selected_branch().frame_count().checked_sub(1) else {
            return;
        };
        let frame = frame.min(last);
        let branch_id = session.selected_branch_id().to_owned();
        let anchor = if extend {
            match self.selection.as_ref() {
                Some(TasTimelineSelection::Rows { anchor, .. }) => (*anchor).min(last),
                Some(TasTimelineSelection::End { .. }) | None => last,
            }
        } else {
            frame
        };
        self.selection = Some(TasTimelineSelection::Rows {
            branch_id,
            anchor,
            active: frame,
        });
    }

    pub(super) fn select_frame_range(
        &mut self,
        session: &TasEditorSession,
        anchor: u64,
        active: u64,
    ) -> Option<u64> {
        self.sync(session);
        let last = session.selected_branch().frame_count().checked_sub(1)?;
        let branch_id = session.selected_branch_id().to_owned();
        let active = active.min(last);
        self.selection = Some(TasTimelineSelection::Rows {
            branch_id,
            anchor: anchor.min(last),
            active,
        });
        Some(active)
    }

    pub(super) fn navigate(
        &mut self,
        session: &TasEditorSession,
        navigation: TasTimelineNavigation,
        extend: bool,
    ) -> u64 {
        self.sync(session);
        let frame_count = session.selected_branch().frame_count();
        let Some(last) = frame_count.checked_sub(1) else {
            return 0;
        };
        let active = match self.selection.as_ref() {
            Some(TasTimelineSelection::Rows { active, .. }) => (*active).min(last),
            Some(TasTimelineSelection::End { .. }) | None => session.cursor().min(last),
        };
        if !extend && navigation == TasTimelineNavigation::End {
            self.selection = Some(TasTimelineSelection::End {
                branch_id: session.selected_branch_id().to_owned(),
            });
            return frame_count;
        }
        let target = match navigation {
            TasTimelineNavigation::Previous => active.saturating_sub(1),
            TasTimelineNavigation::Next => active.saturating_add(1).min(last),
            TasTimelineNavigation::Start => 0,
            TasTimelineNavigation::End => last,
        };
        let anchor = if extend {
            match self.selection.as_ref() {
                Some(TasTimelineSelection::Rows { anchor, .. }) => (*anchor).min(last),
                Some(TasTimelineSelection::End { .. }) | None => active,
            }
        } else {
            target
        };
        self.selection = Some(TasTimelineSelection::Rows {
            branch_id: session.selected_branch_id().to_owned(),
            anchor,
            active: target,
        });
        target
    }

    pub(super) fn select_all(&mut self, session: &TasEditorSession) {
        let branch_id = session.selected_branch_id().to_owned();
        self.selection = Some(
            session
                .selected_branch()
                .frame_count()
                .checked_sub(1)
                .map_or(
                    TasTimelineSelection::End {
                        branch_id: branch_id.clone(),
                    },
                    |last| TasTimelineSelection::Rows {
                        branch_id,
                        anchor: 0,
                        active: last,
                    },
                ),
        );
    }

    pub(super) fn collapse_to_cursor(&mut self, session: &TasEditorSession) {
        let branch_id = session.selected_branch_id().to_owned();
        let cursor = session.cursor();
        self.selection = if cursor < session.selected_branch().frame_count() {
            Some(TasTimelineSelection::Rows {
                branch_id,
                anchor: cursor,
                active: cursor,
            })
        } else {
            Some(TasTimelineSelection::End { branch_id })
        };
    }

    pub(super) fn select_range(&mut self, session: &TasEditorSession, start: u64, end: u64) {
        let frame_count = session.selected_branch().frame_count();
        if start >= end || end > frame_count {
            self.collapse_to_cursor(session);
            return;
        }
        self.selection = Some(TasTimelineSelection::Rows {
            branch_id: session.selected_branch_id().to_owned(),
            anchor: start,
            active: end - 1,
        });
    }

    pub(super) fn selected_range(&mut self, session: &TasEditorSession) -> Option<(u64, u64)> {
        self.snapshot(session)
            .map(|selection| (selection.start, selection.end))
    }

    pub(super) fn active_cursor(&mut self, session: &TasEditorSession) -> u64 {
        self.sync(session);
        match self.selection.as_ref() {
            Some(TasTimelineSelection::Rows { active, .. }) => *active,
            Some(TasTimelineSelection::End { .. }) | None => {
                session.selected_branch().frame_count()
            }
        }
    }

    pub(super) fn snapshot(&mut self, session: &TasEditorSession) -> Option<TasInputSelection> {
        self.sync(session);
        match self.selection.as_ref()? {
            TasTimelineSelection::End { .. } => None,
            TasTimelineSelection::Rows {
                branch_id,
                anchor,
                active,
            } => Some(TasInputSelection {
                branch_id: branch_id.clone(),
                start: (*anchor).min(*active),
                end: (*anchor).max(*active) + 1,
            }),
        }
    }

    fn sync(&mut self, session: &TasEditorSession) {
        let branch_id = session.selected_branch_id();
        let frame_count = session.selected_branch().frame_count();
        let branch_changed = match self.selection.as_ref() {
            Some(TasTimelineSelection::End {
                branch_id: selected,
            })
            | Some(TasTimelineSelection::Rows {
                branch_id: selected,
                ..
            }) => selected != branch_id,
            None => true,
        };
        if branch_changed {
            self.collapse_to_cursor(session);
            return;
        }
        let Some(last) = frame_count.checked_sub(1) else {
            self.selection = Some(TasTimelineSelection::End {
                branch_id: branch_id.to_owned(),
            });
            return;
        };
        if let Some(TasTimelineSelection::Rows { anchor, active, .. }) = self.selection.as_mut() {
            *anchor = (*anchor).min(last);
            *active = (*active).min(last);
        }
    }
}

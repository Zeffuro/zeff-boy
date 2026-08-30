use std::time::Duration;

use super::TasEditorWindowState;
use crate::tas_project::TasEditorAutosaveTick;

impl TasEditorWindowState {
    pub(crate) fn tick_periodic_autosave(&mut self) {
        self.tick_periodic_autosave_at(self.autosave_clock.elapsed());
    }

    pub(crate) fn autosave_before_shutdown(
        &mut self,
    ) -> anyhow::Result<Option<crate::tas_project::TasAutosaveSave>> {
        self.discard_recording_draft()?;
        let Some(session) = self.session.as_mut() else {
            return Ok(None);
        };
        if !session.is_dirty() {
            return Ok(None);
        }
        session.autosave_if_changed()
    }

    fn tick_periodic_autosave_at(&mut self, now: Duration) {
        if self.recording.is_some() {
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match self.autosave_scheduler.tick(now, session) {
            Ok(TasEditorAutosaveTick::Saved(saved)) => {
                self.message = Some((
                    false,
                    format!(
                        "Periodic autosave generation {} written to {}",
                        saved.generation,
                        saved.path.display()
                    ),
                ));
            }
            Ok(
                TasEditorAutosaveTick::Armed
                | TasEditorAutosaveTick::Waiting
                | TasEditorAutosaveTick::Current,
            ) => {}
            Err(error) => {
                self.message = Some((true, format!("Periodic autosave deferred: {error:#}")));
            }
        }
    }

    #[cfg(test)]
    pub(super) fn test_tick_periodic_autosave_at(&mut self, now: Duration) {
        self.tick_periodic_autosave_at(now);
    }
}

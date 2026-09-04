#![cfg(not(target_arch = "wasm32"))]

use std::time::Duration;

use anyhow::{Result, bail};

use super::{TasAutosaveSave, TasEditorSession};

pub const DEFAULT_TAS_EDITOR_AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);
pub const DEFAULT_TAS_EDITOR_AUTOSAVE_RETRY_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TasEditorAutosaveTick {
    Armed,
    Waiting,
    Current,
    Saved(TasAutosaveSave),
}

#[derive(Clone, Debug)]
pub struct TasEditorAutosaveScheduler {
    interval: Duration,
    retry_delay: Duration,
    last_now: Option<Duration>,
    next_due: Option<Duration>,
}

impl Default for TasEditorAutosaveScheduler {
    fn default() -> Self {
        Self::new(
            DEFAULT_TAS_EDITOR_AUTOSAVE_INTERVAL,
            DEFAULT_TAS_EDITOR_AUTOSAVE_RETRY_DELAY,
        )
        .expect("default TAS editor autosave durations are non-zero")
    }
}

impl TasEditorAutosaveScheduler {
    pub fn new(interval: Duration, retry_delay: Duration) -> Result<Self> {
        if interval.is_zero() || retry_delay.is_zero() {
            bail!("TAS editor autosave intervals must be non-zero");
        }
        Ok(Self {
            interval,
            retry_delay,
            last_now: None,
            next_due: None,
        })
    }

    pub fn reset(&mut self) {
        self.last_now = None;
        self.next_due = None;
    }

    pub fn next_due(&self) -> Option<Duration> {
        self.next_due
    }

    pub fn tick(
        &mut self,
        now: Duration,
        session: &mut TasEditorSession,
    ) -> Result<TasEditorAutosaveTick> {
        if self.last_now.is_some_and(|previous| now < previous) {
            bail!("TAS editor autosave clock moved backwards");
        }
        self.last_now = Some(now);

        let Some(next_due) = self.next_due else {
            self.next_due = Some(schedule_after(now, self.interval));
            return Ok(TasEditorAutosaveTick::Armed);
        };
        if now < next_due {
            return Ok(TasEditorAutosaveTick::Waiting);
        }

        match session.autosave_if_changed() {
            Ok(Some(saved)) => {
                self.next_due = Some(schedule_after(now, self.interval));
                Ok(TasEditorAutosaveTick::Saved(saved))
            }
            Ok(None) => {
                self.next_due = Some(schedule_after(now, self.interval));
                Ok(TasEditorAutosaveTick::Current)
            }
            Err(error) => {
                self.next_due = Some(schedule_after(now, self.retry_delay));
                Err(error)
            }
        }
    }
}

fn schedule_after(now: Duration, delay: Duration) -> Duration {
    now.checked_add(delay).unwrap_or(Duration::MAX)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::tas_project::{TasAutosaveConfig, TasAutosaveStore, TasSeekStateCache};

    fn session(root: &Path) -> TasEditorSession {
        let manual = root.join("movie.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual, TasAutosaveConfig::default()).unwrap();
        let seek_cache = TasSeekStateCache::open(root.join("seek-cache")).unwrap();
        TasEditorSession::new(
            crate::tas_project::tests::project(),
            manual,
            autosaves,
            seek_cache,
        )
        .unwrap()
    }

    #[test]
    fn arms_waits_saves_once_and_returns_to_the_normal_interval() {
        let root = crate::test_support::test_directory("tas-editor-periodic-autosave").unwrap();
        let mut session = session(root.path());
        let mut scheduler =
            TasEditorAutosaveScheduler::new(Duration::from_secs(10), Duration::from_secs(2))
                .unwrap();

        assert_eq!(
            scheduler
                .tick(Duration::from_secs(3), &mut session)
                .unwrap(),
            TasEditorAutosaveTick::Armed
        );
        assert_eq!(scheduler.next_due(), Some(Duration::from_secs(13)));
        assert_eq!(
            scheduler
                .tick(Duration::from_secs(12), &mut session)
                .unwrap(),
            TasEditorAutosaveTick::Waiting
        );
        let TasEditorAutosaveTick::Saved(saved) = scheduler
            .tick(Duration::from_secs(13), &mut session)
            .unwrap()
        else {
            panic!("first due tick should publish the unsaved project");
        };
        assert!(saved.path.exists());
        assert_eq!(scheduler.next_due(), Some(Duration::from_secs(23)));
        assert_eq!(
            scheduler
                .tick(Duration::from_secs(23), &mut session)
                .unwrap(),
            TasEditorAutosaveTick::Current
        );
        assert_eq!(scheduler.next_due(), Some(Duration::from_secs(33)));
    }

    #[test]
    fn write_failure_uses_retry_delay_without_advancing_the_autosave_witness() {
        let root = crate::test_support::test_directory("tas-editor-autosave-retry").unwrap();
        let mut session = session(root.path());
        let autosave_directory = session.autosave_directory().to_path_buf();
        let displaced = root.path().join("displaced-autosaves");
        std::fs::rename(&autosave_directory, &displaced).unwrap();
        std::fs::create_dir(&autosave_directory).unwrap();
        let mut scheduler =
            TasEditorAutosaveScheduler::new(Duration::from_secs(10), Duration::from_secs(2))
                .unwrap();

        scheduler.tick(Duration::ZERO, &mut session).unwrap();
        assert!(
            scheduler
                .tick(Duration::from_secs(10), &mut session)
                .is_err()
        );
        assert_eq!(session.last_autosaved_generation(), None);
        assert_eq!(scheduler.next_due(), Some(Duration::from_secs(12)));
        assert_eq!(
            scheduler
                .tick(Duration::from_secs(11), &mut session)
                .unwrap(),
            TasEditorAutosaveTick::Waiting
        );
        assert!(
            scheduler
                .tick(Duration::from_secs(12), &mut session)
                .is_err()
        );
        assert_eq!(scheduler.next_due(), Some(Duration::from_secs(14)));
    }

    #[test]
    fn reset_rearms_and_backwards_time_is_rejected_without_rescheduling() {
        let root = crate::test_support::test_directory("tas-editor-autosave-clock").unwrap();
        let mut session = session(root.path());
        let mut scheduler =
            TasEditorAutosaveScheduler::new(Duration::from_secs(10), Duration::from_secs(2))
                .unwrap();

        scheduler
            .tick(Duration::from_secs(5), &mut session)
            .unwrap();
        assert!(
            scheduler
                .tick(Duration::from_secs(4), &mut session)
                .is_err()
        );
        assert_eq!(scheduler.next_due(), Some(Duration::from_secs(15)));
        scheduler.reset();
        assert_eq!(
            scheduler
                .tick(Duration::from_secs(100), &mut session)
                .unwrap(),
            TasEditorAutosaveTick::Armed
        );
        assert_eq!(scheduler.next_due(), Some(Duration::from_secs(110)));
    }

    #[test]
    fn zero_intervals_are_rejected() {
        assert!(TasEditorAutosaveScheduler::new(Duration::ZERO, Duration::from_secs(1)).is_err());
        assert!(TasEditorAutosaveScheduler::new(Duration::from_secs(1), Duration::ZERO).is_err());
    }
}

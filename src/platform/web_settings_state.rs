use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SettingsStorageStatus {
    pub(crate) generation: u64,
    pub(crate) pending: usize,
    pub(crate) latest_requested_sequence: u64,
    pub(crate) persisted_sequence: u64,
    pub(crate) persisted_json: Option<String>,
    pub(crate) write_protected: bool,
    pub(crate) error_sequence: Option<u64>,
    pub(crate) error: Option<String>,
    pub(crate) conflicting_json: Option<String>,
    pub(crate) conflict_revision: Option<u64>,
}

#[derive(Debug, Default)]
pub(super) struct SettingsStorageTracker {
    status: SettingsStorageStatus,
    pending: BTreeSet<u64>,
}

impl SettingsStorageTracker {
    pub(super) fn initialize(&mut self, persisted_json: Option<String>, write_protected: bool) {
        self.status.persisted_json = persisted_json;
        self.status.write_protected = write_protected;
        self.bump_generation();
    }

    pub(super) fn initialization_failed(&mut self, message: String) {
        self.status.error_sequence = None;
        self.status.error = Some(message);
        self.status.conflicting_json = None;
        self.status.conflict_revision = None;
        self.bump_generation();
    }

    pub(super) fn mark_write_protected(&mut self) {
        if !self.status.write_protected {
            self.status.write_protected = true;
            self.bump_generation();
        }
    }

    pub(super) fn generation(&self) -> u64 {
        self.status.generation
    }

    pub(super) fn schedule(&mut self) -> Option<u64> {
        let sequence = self.status.latest_requested_sequence.checked_add(1)?;
        self.status.latest_requested_sequence = sequence;
        self.pending.insert(sequence);
        self.refresh_pending();
        self.bump_generation();
        Some(sequence)
    }

    pub(super) fn persisted(&mut self, sequence: u64, json: String) {
        self.pending.remove(&sequence);
        if sequence >= self.status.persisted_sequence {
            self.status.persisted_sequence = sequence;
            self.status.persisted_json = Some(json);
        }
        if self
            .status
            .error_sequence
            .is_some_and(|failed| failed <= sequence)
        {
            self.clear_error();
        }
        self.refresh_pending();
        self.bump_generation();
    }

    pub(super) fn failed(
        &mut self,
        sequence: u64,
        message: String,
        conflict: Option<(u64, String)>,
    ) {
        self.pending.remove(&sequence);
        if sequence > self.status.persisted_sequence
            && self
                .status
                .error_sequence
                .is_none_or(|current| sequence >= current)
        {
            self.status.error_sequence = Some(sequence);
            self.status.error = Some(message);
            if let Some((revision, json)) = conflict {
                self.status.conflict_revision = Some(revision);
                self.status.conflicting_json = Some(json);
            } else {
                self.status.conflict_revision = None;
                self.status.conflicting_json = None;
            }
        }
        self.refresh_pending();
        self.bump_generation();
    }

    pub(super) fn accept_conflict(&mut self) -> Option<(u64, String)> {
        let revision = self.status.conflict_revision.take()?;
        let json = self.status.conflicting_json.take()?;
        self.status.persisted_json = Some(json.clone());
        self.clear_error();
        self.bump_generation();
        Some((revision, json))
    }

    pub(super) fn snapshot(&self) -> SettingsStorageStatus {
        self.status.clone()
    }

    fn clear_error(&mut self) {
        self.status.error_sequence = None;
        self.status.error = None;
        self.status.conflicting_json = None;
        self.status.conflict_revision = None;
    }

    fn refresh_pending(&mut self) {
        self.status.pending = self.pending.len();
    }

    fn bump_generation(&mut self) {
        self.status.generation = self.status.generation.wrapping_add(1).max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_failure_cannot_replace_newer_durable_result() {
        let mut tracker = SettingsStorageTracker::default();
        let first = tracker.schedule().unwrap();
        let second = tracker.schedule().unwrap();
        tracker.persisted(second, "new".to_string());
        tracker.failed(first, "stale failure".to_string(), None);

        let status = tracker.snapshot();
        assert_eq!(status.persisted_sequence, second);
        assert_eq!(status.persisted_json.as_deref(), Some("new"));
        assert_eq!(status.error, None);
        assert_eq!(status.pending, 0);
    }

    #[test]
    fn newer_success_supersedes_an_earlier_storage_error() {
        let mut tracker = SettingsStorageTracker::default();
        let first = tracker.schedule().unwrap();
        tracker.failed(first, "quota".to_string(), None);
        assert_eq!(tracker.snapshot().error.as_deref(), Some("quota"));

        let second = tracker.schedule().unwrap();
        assert_eq!(tracker.snapshot().error.as_deref(), Some("quota"));
        tracker.persisted(second, "saved".to_string());
        assert_eq!(tracker.snapshot().error, None);
    }

    #[test]
    fn conflict_requires_explicit_acceptance_before_rebasing() {
        let mut tracker = SettingsStorageTracker::default();
        tracker.initialize(Some("original".to_string()), false);
        let sequence = tracker.schedule().unwrap();
        tracker.failed(
            sequence,
            "changed in another tab".to_string(),
            Some((7, "remote".to_string())),
        );

        let status = tracker.snapshot();
        assert_eq!(status.persisted_json.as_deref(), Some("original"));
        assert_eq!(status.conflicting_json.as_deref(), Some("remote"));
        assert_eq!(tracker.accept_conflict(), Some((7, "remote".to_string())));
        let accepted = tracker.snapshot();
        assert_eq!(accepted.persisted_json.as_deref(), Some("remote"));
        assert_eq!(accepted.error, None);
    }

    #[test]
    fn pending_count_tracks_each_requested_transaction() {
        let mut tracker = SettingsStorageTracker::default();
        let first = tracker.schedule().unwrap();
        let second = tracker.schedule().unwrap();
        assert_eq!(tracker.snapshot().pending, 2);
        tracker.persisted(first, "one".to_string());
        assert_eq!(tracker.snapshot().pending, 1);
        tracker.failed(second, "blocked".to_string(), None);
        assert_eq!(tracker.snapshot().pending, 0);
    }

    #[test]
    fn initialization_failure_keeps_fallback_json_and_reports_an_error() {
        let mut tracker = SettingsStorageTracker::default();
        tracker.initialize(Some("legacy".to_string()), false);
        tracker.initialization_failed("IndexedDB blocked".to_string());

        let status = tracker.snapshot();
        assert_eq!(status.persisted_json.as_deref(), Some("legacy"));
        assert_eq!(status.error.as_deref(), Some("IndexedDB blocked"));
        assert_eq!(status.error_sequence, None);
    }

    #[test]
    fn future_format_protection_changes_generation_once() {
        let mut tracker = SettingsStorageTracker::default();
        let before = tracker.generation();
        tracker.mark_write_protected();
        let protected = tracker.snapshot();
        assert!(protected.write_protected);
        assert!(protected.generation > before);

        tracker.mark_write_protected();
        assert_eq!(tracker.generation(), protected.generation);
    }
}

#![cfg(not(target_arch = "wasm32"))]

use std::collections::VecDeque;

use anyhow::Result;

use super::{TasDigest, TasProject};

pub const MAX_TAS_EDITOR_HISTORY_ENTRIES: usize = 32;
pub const MAX_TAS_EDITOR_HISTORY_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TasEditorProjectWitness {
    pub(super) generation: u64,
    pub(super) project_sha256: TasDigest,
}

pub(super) fn project_sha256(project: &TasProject) -> Result<TasDigest> {
    project.editor_content_sha256()
}

pub(super) fn project_witness(project: &TasProject) -> Result<TasEditorProjectWitness> {
    Ok(TasEditorProjectWitness {
        generation: project.edit_generation(),
        project_sha256: project_sha256(project)?,
    })
}

#[derive(Clone, Debug)]
pub(super) struct TasEditorHistoryEntry {
    pub(super) project_bytes: Vec<u8>,
    pub(super) selected_branch_id: String,
    pub(super) cursor: u64,
}

impl TasEditorHistoryEntry {
    fn retained_bytes(&self) -> usize {
        self.project_bytes
            .len()
            .saturating_add(self.selected_branch_id.len())
    }
}

#[derive(Clone, Debug)]
pub(super) struct TasEditorHistoryStack {
    pub(super) entries: VecDeque<TasEditorHistoryEntry>,
    retained_bytes: usize,
    max_entries: usize,
    pub(super) max_bytes: usize,
}

impl TasEditorHistoryStack {
    fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            retained_bytes: 0,
            max_entries,
            max_bytes,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.retained_bytes = 0;
    }

    pub(super) fn can_retain(&self, entry: &TasEditorHistoryEntry) -> bool {
        self.max_entries != 0 && entry.retained_bytes() <= self.max_bytes
    }

    pub(super) fn push(&mut self, entry: TasEditorHistoryEntry) {
        debug_assert!(self.can_retain(&entry));
        self.retained_bytes = self.retained_bytes.saturating_add(entry.retained_bytes());
        self.entries.push_back(entry);
        while self.entries.len() > self.max_entries || self.retained_bytes > self.max_bytes {
            let removed = self
                .entries
                .pop_front()
                .expect("an over-budget TAS history stack must contain an entry");
            self.retained_bytes = self.retained_bytes.saturating_sub(removed.retained_bytes());
        }
    }

    pub(super) fn pop(&mut self) -> Option<TasEditorHistoryEntry> {
        let entry = self.entries.pop_back()?;
        self.retained_bytes = self.retained_bytes.saturating_sub(entry.retained_bytes());
        Some(entry)
    }

    pub(super) fn last(&self) -> Option<&TasEditorHistoryEntry> {
        self.entries.back()
    }
}

#[derive(Clone, Debug)]
pub(super) struct TasEditorHistory {
    pub(super) undo: TasEditorHistoryStack,
    pub(super) redo: TasEditorHistoryStack,
}

impl TasEditorHistory {
    pub(super) fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            undo: TasEditorHistoryStack::new(max_entries, max_bytes),
            redo: TasEditorHistoryStack::new(max_entries, max_bytes),
        }
    }

    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

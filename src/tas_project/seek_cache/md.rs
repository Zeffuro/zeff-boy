use std::collections::VecDeque;
use std::mem::size_of;

use anyhow::{Result, ensure};
use zeff_md_core::emulator::rollback::{MdRetainedAllocations, MdRetainedPayload};

use super::{MAX_SEEK_CACHE_IDENTITY_CHECKS_PER_LOAD, TasSeekCacheIdentity, TasSeekStateCache};
use crate::emu_backend::EmuBackend;
use crate::emu_backend::md::tas::{MdTasEditorCheckpoint, MdTasEditorOwner};

struct MdEntry {
    identity: TasSeekCacheIdentity,
    checkpoint: MdTasEditorCheckpoint,
}

#[derive(Default)]
pub(super) struct MdSeekCache {
    entries: VecDeque<MdEntry>,
    last_payload: Option<MdRetainedPayload>,
    last_bytes: u64,
}

impl std::fmt::Debug for MdSeekCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdSeekCache")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl TasSeekStateCache {
    pub(crate) fn restore_md_newest(
        &self,
        owner: &MdTasEditorOwner,
        backend: &mut EmuBackend,
        target: u64,
        mut expected: impl FnMut(u64) -> Result<TasSeekCacheIdentity>,
    ) -> Result<Option<u64>> {
        self.validate_root()?;
        let cache = self
            .md
            .lock()
            .map_err(|_| anyhow::anyhow!("MD cache lock poisoned"))?;
        let mut candidates: Vec<_> = cache
            .entries
            .iter()
            .filter(|entry| entry.identity.cursor <= target && entry.checkpoint.belongs_to(owner))
            .collect();
        candidates.sort_by_key(|entry| std::cmp::Reverse(entry.identity.cursor));
        for entry in candidates
            .into_iter()
            .take(MAX_SEEK_CACHE_IDENTITY_CHECKS_PER_LOAD)
        {
            if entry.identity != expected(entry.identity.cursor)? {
                continue;
            }
            ensure!(
                entry.checkpoint.frame() == entry.identity.cursor,
                "MD cache frame proof differs"
            );
            let EmuBackend::Md(md) = backend else {
                anyhow::bail!("MD typed cache requires MD backend")
            };
            md.restore_tas_editor(owner, &entry.checkpoint)?;
            return Ok(Some(entry.identity.cursor));
        }
        Ok(None)
    }

    pub(crate) fn store_md(
        &self,
        identity: TasSeekCacheIdentity,
        checkpoint: MdTasEditorCheckpoint,
        owner: &MdTasEditorOwner,
        baseline: &MdTasEditorCheckpoint,
        backend: &EmuBackend,
    ) -> Result<()> {
        self.validate_root()?;
        checkpoint.validate(owner)?;
        ensure!(
            checkpoint.frame() == identity.cursor,
            "MD cache cursor differs from runtime"
        );
        let mut single = MdRetainedAllocations::new();
        checkpoint.visit_retained(&mut single)?;
        let state_bytes =
            single.finish()?.requested_payload_bytes() + size_of::<MdTasEditorCheckpoint>() as u64;
        ensure!(
            state_bytes <= self.limits.max_state_bytes as u64,
            "MD typed cursor exceeds state budget"
        );
        let disk_entries = self.collect_entries()?;
        let disk_bytes = disk_entries.iter().try_fold(0u64, |sum, entry| {
            sum.checked_add(entry.bytes)
                .ok_or_else(|| anyhow::anyhow!("MD disk reservation overflow"))
        })?;
        let mut cache = self
            .md
            .lock()
            .map_err(|_| anyhow::anyhow!("MD cache lock poisoned"))?;
        cache.entries.try_reserve(1)?;
        cache
            .entries
            .retain(|entry| entry.identity != identity || !entry.checkpoint.belongs_to(owner));
        cache.entries.push_back(MdEntry {
            identity,
            checkpoint,
        });
        loop {
            let (bytes, graph) = cache.retained_bytes(owner, baseline, backend)?;
            cache.last_payload = Some(graph);
            cache.last_bytes = bytes;
            if cache.entries.len().saturating_add(disk_entries.len()) <= self.limits.max_entries
                && bytes.saturating_add(disk_bytes) <= self.limits.max_bytes
            {
                return Ok(());
            }
            ensure!(
                cache.entries.pop_front().is_some(),
                "MD baseline and active runtime exceed cache budget"
            );
        }
    }

    pub(super) fn clear_md_runtime_entries(&self) -> Result<()> {
        let mut cache = self
            .md
            .lock()
            .map_err(|_| anyhow::anyhow!("MD cache lock poisoned"))?;
        *cache = MdSeekCache::default();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn corrupt_md_newest_for_test(&self) -> Result<()> {
        let mut cache = self
            .md
            .lock()
            .map_err(|_| anyhow::anyhow!("MD cache lock poisoned"))?;
        let entry = cache
            .entries
            .back_mut()
            .ok_or_else(|| anyhow::anyhow!("MD cache is empty"))?;
        entry.checkpoint.corrupt_proof_for_test();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn md_retained_for_test(&self) -> Result<(usize, u64, MdRetainedPayload)> {
        let cache = self
            .md
            .lock()
            .map_err(|_| anyhow::anyhow!("MD cache lock poisoned"))?;
        Ok((
            cache.entries.len(),
            cache.last_bytes,
            cache
                .last_payload
                .clone()
                .ok_or_else(|| anyhow::anyhow!("MD cache has no payload observation"))?,
        ))
    }
}

impl MdSeekCache {
    fn retained_bytes(
        &self,
        owner: &MdTasEditorOwner,
        baseline: &MdTasEditorCheckpoint,
        backend: &EmuBackend,
    ) -> Result<(u64, MdRetainedPayload)> {
        let mut retained = MdRetainedAllocations::new();
        baseline.visit_retained(&mut retained)?;
        let EmuBackend::Md(md) = backend else {
            anyhow::bail!("MD cache accounting requires MD backend")
        };
        retained.visit_emulator(md.core())?;
        for entry in &self.entries {
            entry.checkpoint.visit_retained(&mut retained)?;
        }
        let graph = retained.finish()?;
        let metadata = self
            .entries
            .capacity()
            .checked_mul(size_of::<MdEntry>())
            .and_then(|bytes| bytes.checked_add(size_of::<std::sync::Mutex<Self>>()))
            .and_then(|bytes| bytes.checked_add(size_of::<MdTasEditorCheckpoint>()))
            .and_then(|bytes| bytes.checked_add(owner.retained_metadata_bytes()))
            .ok_or_else(|| anyhow::anyhow!("MD cache metadata size overflow"))?;
        let text = self.entries.iter().try_fold(0usize, |sum, entry| {
            sum.checked_add(entry.identity.state_format_compatibility_id.capacity())
                .ok_or_else(|| anyhow::anyhow!("MD cache identity size overflow"))
        })?;
        let bytes = graph
            .requested_payload_bytes()
            .checked_add(u64::try_from(
                metadata
                    .checked_add(text)
                    .ok_or_else(|| anyhow::anyhow!("MD cache size overflow"))?,
            )?)
            .and_then(|bytes| bytes.checked_add(graph.collector_capacity_bytes()))
            .ok_or_else(|| anyhow::anyhow!("MD cache payload size overflow"))?;
        Ok((bytes, graph))
    }
}

use super::*;
use crate::emu_backend::md::MdBackend;
use crate::emu_backend::md::tas::{MdTasLiveGuard, MdTasLiveLease, MdTasLiveSnapshot};
use anyhow::{Result, ensure};
use std::mem::size_of;
use zeff_md_core::emulator::rollback::{MdRetainedAllocations, MdRollbackSnapshot};

#[derive(Default)]
pub(super) struct MdWorkerCache {
    reservation: usize,
    hits: usize,
}

impl WorkerTasStateCache {
    pub(in crate::emu_thread::emu_loop::tas_control) fn clear_md(&mut self) {
        self.entries
            .retain(|entry| matches!(entry.state, CacheState::Bytes(_)));
    }

    pub(in crate::emu_thread::emu_loop::tas_control) fn reserve_md(&mut self, bytes: usize) {
        self.md.reservation = bytes;
    }

    pub(in crate::emu_thread::emu_loop::tas_control) fn get_md(
        &mut self,
        profile: TasExecutionProfile,
        schema: &'static str,
        proofs: &[TasExecutionCacheProof],
    ) -> Option<(TasExecutionCacheProof, Arc<MdTasLiveSnapshot>)> {
        for proof in proofs {
            let key = CacheKey {
                profile,
                state_format_compatibility_id: schema,
                proof: *proof,
            };
            let Some(index) = self
                .entries
                .iter()
                .position(|entry| entry.key == key && matches!(entry.state, CacheState::Md(_)))
            else {
                continue;
            };
            let entry = self.entries.remove(index)?;
            let CacheState::Md(state) = &entry.state else {
                unreachable!()
            };
            let state = Arc::clone(state);
            self.entries.push_back(entry);
            self.md.hits = self.md.hits.saturating_add(1);
            return Some((*proof, state));
        }
        None
    }

    pub(in crate::emu_thread::emu_loop::tas_control) fn md_retained_bytes(
        &self,
        backend: &MdBackend,
        lease: &MdTasLiveLease,
        transient: &[&MdTasLiveSnapshot],
    ) -> Result<u64> {
        let graph = self.md_retained_graph(backend, transient)?;
        let count = self
            .entries
            .iter()
            .filter(|entry| matches!(entry.state, CacheState::Md(_)))
            .count();
        // Core visits include their layout; outer proof/owner and guard fields are additional.
        let wrappers = count
            .checked_mul(size_of::<MdTasLiveSnapshot>() - size_of::<MdRollbackSnapshot>())
            .and_then(|n| {
                n.checked_add(
                    transient.len()
                        * (size_of::<MdTasLiveGuard>() - size_of::<MdRollbackSnapshot>()),
                )
            })
            .ok_or_else(|| anyhow::anyhow!("MD cache wrapper size overflow"))?;
        let metadata = backend
            .tas_live_metadata_bytes(lease)?
            .checked_add(self.entries.capacity() * size_of::<CacheEntry>())
            .and_then(|n| n.checked_add(wrappers))
            .and_then(|n| n.checked_add(self.md.reservation))
            .and_then(|n| n.checked_add(size_of::<Self>()))
            .ok_or_else(|| anyhow::anyhow!("MD cache metadata size overflow"))?;
        let byte_payloads = self.retained_byte_payloads()?;
        graph
            .requested_payload_bytes()
            .checked_add(graph.collector_capacity_bytes())
            .and_then(|n| n.checked_add(metadata as u64))
            .and_then(|n| n.checked_add(byte_payloads))
            .ok_or_else(|| anyhow::anyhow!("MD cache graph size overflow"))
    }

    fn retained_byte_payloads(&self) -> Result<u64> {
        let mut total = 0u64;
        for (index, entry) in self.entries.iter().enumerate() {
            let CacheState::Bytes(state) = &entry.state else {
                continue;
            };
            if self.entries.iter().take(index).any(|prior| {
                matches!(&prior.state, CacheState::Bytes(prior) if Arc::ptr_eq(prior, state))
            }) {
                continue;
            }
            let payload = u64::try_from(size_of::<CachedTasState>())?
                .checked_add(u64::try_from(state.bytes.capacity())?)
                .ok_or_else(|| anyhow::anyhow!("MD peer byte payload size overflow"))?;
            total = total
                .checked_add(payload)
                .ok_or_else(|| anyhow::anyhow!("MD peer byte payload total overflow"))?;
        }
        Ok(total)
    }

    pub(in crate::emu_thread::emu_loop::tas_control) fn md_retained_graph(
        &self,
        backend: &MdBackend,
        transient: &[&MdTasLiveSnapshot],
    ) -> Result<zeff_md_core::emulator::rollback::MdRetainedPayload> {
        let mut graph = MdRetainedAllocations::new();
        backend.visit_rollback_core_allocations(&mut graph)?;
        for entry in &self.entries {
            if let CacheState::Md(state) = &entry.state {
                state.visit_retained(&mut graph)?;
            }
        }
        for state in transient {
            state.visit_retained(&mut graph)?;
        }
        graph.finish()
    }

    pub(in crate::emu_thread::emu_loop::tas_control) fn bound_md(
        &mut self,
        backend: &MdBackend,
        lease: &MdTasLiveLease,
        transient: &[&MdTasLiveSnapshot],
    ) -> Result<()> {
        loop {
            if self.entries.len() <= MAX_ENTRIES
                && self.md_retained_bytes(backend, lease, transient)? <= MAX_BYTES as u64
            {
                return Ok(());
            }
            let entry = self.entries.pop_front().ok_or_else(|| {
                anyhow::anyhow!("MD active and original runtime exceed worker cache bound")
            })?;
            self.total_bytes = self.total_bytes.saturating_sub(entry.byte_len());
        }
    }

    pub(in crate::emu_thread::emu_loop::tas_control) fn insert_md(
        &mut self,
        profile: TasExecutionProfile,
        schema: &'static str,
        proof: TasExecutionCacheProof,
        state: MdTasLiveSnapshot,
    ) -> Result<()> {
        ensure!(
            state.proof().frame() == proof.target_cursor,
            "MD cache cursor differs"
        );
        self.entries.try_reserve(1)?;
        let key = CacheKey {
            profile,
            state_format_compatibility_id: schema,
            proof,
        };
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key)
            && let Some(entry) = self.entries.remove(index)
        {
            self.total_bytes = self.total_bytes.saturating_sub(entry.byte_len());
        }
        self.entries.push_back(CacheEntry {
            key,
            state: CacheState::Md(Arc::new(state)),
        });
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::emu_thread::emu_loop::tas_control) fn md_cache_observation(
        &self,
    ) -> (usize, usize) {
        (
            self.entries
                .iter()
                .filter(|entry| matches!(entry.state, CacheState::Md(_)))
                .count(),
            self.md.hits,
        )
    }

    #[cfg(test)]
    pub(in crate::emu_thread::emu_loop::tas_control) fn corrupt_md_for_test(&mut self) {
        let entry = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| matches!(entry.state, CacheState::Md(_)))
            .unwrap();
        let CacheState::Md(state) = &mut entry.state else {
            unreachable!()
        };
        Arc::get_mut(state)
            .unwrap()
            .corrupt_tas_live_proof_for_test();
    }
}

#[cfg(test)]
impl crate::emu_thread::emu_loop::EmuLoop {
    pub(in crate::emu_thread::emu_loop) fn insert_md_tas_peer_capacity_case_for_test(
        &mut self,
    ) -> u64 {
        let cache = &mut self.tas_control.state_cache;
        let before = cache.retained_byte_payloads().unwrap();
        let mut bytes = Vec::with_capacity(MAX_BYTES + 1);
        bytes.push(0);
        let expected = before + (size_of::<CachedTasState>() + bytes.capacity()) as u64;
        let state = Arc::new(CachedTasState {
            sha256: crate::tas_project::TasDigest::from_bytes(&bytes),
            bytes,
            frame_count: 0,
        });
        for cursor in [900, 901] {
            cache.entries.push_front(CacheEntry {
                key: CacheKey {
                    profile: TasExecutionProfile::DirectNesCartridge,
                    state_format_compatibility_id: "peer-byte-capacity-case",
                    proof: TasExecutionCacheProof {
                        sync_identity_sha256: crate::tas_project::TasDigest::from_bytes(b"peer"),
                        branch_prefix_sha256: crate::tas_project::TasDigest::from_bytes(b"prefix"),
                        target_cursor: cursor,
                    },
                },
                state: CacheState::Bytes(Arc::clone(&state)),
            });
            cache.total_bytes += state.bytes.len();
        }
        assert_eq!(cache.retained_byte_payloads().unwrap(), expected);
        expected
    }
}

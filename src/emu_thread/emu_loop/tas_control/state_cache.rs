use std::collections::VecDeque;

use crate::emu_thread::{TasExecutionCacheProof, TasExecutionProfile, TasExecutionRequest};
use crate::tas_project::TasDigest;

use super::{BackendAuthority, TasControl};

const MAX_ENTRIES: usize = 16;
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CachedTasState {
    pub(super) bytes: Vec<u8>,
    pub(super) sha256: TasDigest,
    pub(super) frame_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheKey {
    profile: TasExecutionProfile,
    state_format_compatibility_id: &'static str,
    proof: TasExecutionCacheProof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheEntry {
    key: CacheKey,
    state: CachedTasState,
}

pub(super) struct WorkerTasStateCache {
    entries: VecDeque<CacheEntry>,
    total_bytes: usize,
}

impl WorkerTasStateCache {
    pub(super) fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            total_bytes: 0,
        }
    }

    pub(super) fn get(
        &mut self,
        profile: TasExecutionProfile,
        state_format_compatibility_id: &'static str,
        proof: TasExecutionCacheProof,
    ) -> Option<CachedTasState> {
        let index = self.entries.iter().position(|entry| {
            entry.key.profile == profile
                && entry.key.state_format_compatibility_id == state_format_compatibility_id
                && entry.key.proof == proof
        })?;
        let entry = self.entries.remove(index)?;
        if TasDigest::from_bytes(&entry.state.bytes) != entry.state.sha256 {
            self.total_bytes = self.total_bytes.saturating_sub(entry.state.bytes.len());
            return None;
        }
        let state = entry.state.clone();
        self.entries.push_back(entry);
        Some(state)
    }

    pub(super) fn get_newest(
        &mut self,
        profile: TasExecutionProfile,
        state_format_compatibility_id: &'static str,
        proofs: &[TasExecutionCacheProof],
    ) -> Option<(TasExecutionCacheProof, CachedTasState)> {
        proofs.iter().copied().find_map(|proof| {
            self.get(profile, state_format_compatibility_id, proof)
                .map(|state| (proof, state))
        })
    }

    pub(super) fn insert(
        &mut self,
        profile: TasExecutionProfile,
        state_format_compatibility_id: &'static str,
        proof: TasExecutionCacheProof,
        state: CachedTasState,
    ) {
        if state.bytes.len() > MAX_BYTES {
            return;
        }
        let key = CacheKey {
            profile,
            state_format_compatibility_id,
            proof,
        };
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key)
            && let Some(replaced) = self.entries.remove(index)
        {
            self.total_bytes = self.total_bytes.saturating_sub(replaced.state.bytes.len());
        }
        self.total_bytes = self.total_bytes.saturating_add(state.bytes.len());
        self.entries.push_back(CacheEntry { key, state });
        while self.entries.len() > MAX_ENTRIES || self.total_bytes > MAX_BYTES {
            let Some(evicted) = self.entries.pop_front() else {
                break;
            };
            self.total_bytes = self.total_bytes.saturating_sub(evicted.state.bytes.len());
        }
    }

    #[cfg(test)]
    pub(super) fn corrupt(
        &mut self,
        profile: TasExecutionProfile,
        state_format_compatibility_id: &'static str,
        proof: TasExecutionCacheProof,
    ) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| {
            entry.key.profile == profile
                && entry.key.state_format_compatibility_id == state_format_compatibility_id
                && entry.key.proof == proof
        }) else {
            return false;
        };
        if let Some(byte) = entry.state.bytes.first_mut() {
            *byte ^= 0xFF;
        } else {
            entry.state.bytes.push(0xFF);
            self.total_bytes = self.total_bytes.saturating_add(1);
        }
        true
    }
}

impl TasControl {
    pub(super) fn cached_state(
        &mut self,
        request: &TasExecutionRequest,
    ) -> Option<(TasExecutionCacheProof, CachedTasState)> {
        let BackendAuthority::Leased {
            lease_id,
            profile,
            state_format_compatibility_id,
            ..
        } = &self.authority
        else {
            return None;
        };
        if *lease_id != request.lease_id || *profile != request.profile {
            return None;
        }
        if let Some(window) = &request.predecessor_window {
            return self.state_cache.get_newest(
                *profile,
                state_format_compatibility_id,
                &window.source_proofs,
            );
        }
        self.state_cache
            .get(*profile, state_format_compatibility_id, request.cache_proof)
            .map(|state| (request.cache_proof, state))
    }

    pub(super) fn candidate_is_cacheable(&self) -> bool {
        self.candidate_cache_proof().is_some()
    }

    pub(super) fn cache_candidate(&mut self, state_bytes: Vec<u8>) {
        let Some(proof) = self.candidate_cache_proof() else {
            return;
        };
        let BackendAuthority::Leased {
            profile,
            state_format_compatibility_id,
            candidate: Some(candidate),
            ..
        } = &self.authority
        else {
            return;
        };
        if candidate.state_sha256 != TasDigest::from_bytes(&state_bytes) {
            return;
        }
        let profile = *profile;
        let state_format_compatibility_id = *state_format_compatibility_id;
        let state = CachedTasState {
            bytes: state_bytes,
            sha256: candidate.state_sha256,
            frame_count: candidate.frame_count,
        };
        self.state_cache
            .insert(profile, state_format_compatibility_id, proof, state);
    }

    fn candidate_cache_proof(&self) -> Option<TasExecutionCacheProof> {
        let BackendAuthority::Leased {
            candidate: Some(candidate),
            intermediate_cache_proofs,
            ..
        } = &self.authority
        else {
            return None;
        };
        if candidate.executed_project_frames == candidate.cache_proof.target_cursor {
            return Some(candidate.cache_proof);
        }
        intermediate_cache_proofs
            .iter()
            .copied()
            .find(|proof| proof.target_cursor == candidate.executed_project_frames)
    }

    #[cfg(test)]
    pub(in crate::emu_thread::emu_loop) fn set_next_lease_id(&mut self, lease_id: u64) {
        self.next_lease_id = lease_id;
    }

    #[cfg(test)]
    pub(in crate::emu_thread::emu_loop) fn corrupt_checkpoint_for_test(&mut self) {
        let BackendAuthority::Leased { checkpoint, .. } = &mut self.authority else {
            panic!("test requires an active TAS lease");
        };
        checkpoint.state_bytes = vec![0xFF];
    }

    #[cfg(test)]
    pub(in crate::emu_thread::emu_loop) fn corrupt_cached_state_for_test(
        &mut self,
        proof: TasExecutionCacheProof,
    ) -> bool {
        let BackendAuthority::Leased {
            profile,
            state_format_compatibility_id,
            ..
        } = &self.authority
        else {
            return false;
        };
        self.state_cache
            .corrupt(*profile, state_format_compatibility_id, proof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof(cursor: u64) -> TasExecutionCacheProof {
        TasExecutionCacheProof {
            sync_identity_sha256: TasDigest([0x11; 32]),
            branch_prefix_sha256: TasDigest([cursor as u8; 32]),
            target_cursor: cursor,
        }
    }

    fn state(byte: u8) -> CachedTasState {
        CachedTasState {
            bytes: vec![byte],
            sha256: TasDigest::from_bytes(&[byte]),
            frame_count: u64::from(byte),
        }
    }

    #[test]
    fn exact_proof_and_schema_are_required_and_recent_entries_survive_eviction() {
        let mut cache = WorkerTasStateCache::new();
        for cursor in 0..MAX_ENTRIES as u64 {
            cache.insert(
                TasExecutionProfile::DirectNesCartridge,
                "nes-v11",
                proof(cursor),
                state(cursor as u8),
            );
        }
        assert_eq!(
            cache.get(TasExecutionProfile::DirectNesCartridge, "nes-v11", proof(0)),
            Some(state(0))
        );
        cache.insert(
            TasExecutionProfile::DirectNesCartridge,
            "nes-v11",
            proof(MAX_ENTRIES as u64),
            state(MAX_ENTRIES as u8),
        );

        assert!(
            cache
                .get(TasExecutionProfile::DirectNesCartridge, "nes-v11", proof(1))
                .is_none()
        );
        assert!(
            cache
                .get(TasExecutionProfile::DirectNesCartridge, "nes-v12", proof(0))
                .is_none()
        );
        assert!(
            cache
                .get(
                    TasExecutionProfile::DirectGbCartridgeDmg,
                    "nes-v11",
                    proof(0)
                )
                .is_none()
        );
    }
}

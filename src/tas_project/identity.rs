use anyhow::{Result, bail};
use sha2::{Digest as _, Sha256};
use zeff_emu_common::replay::{
    encode_canonical_replay_event_stream, encode_replay_event_stream, encode_replay_start_metadata,
};

use super::model::{TasDigest, TasInputSpan, TasProject, TasSeekCacheIdentity};

const CACHE_FORMAT_VERSION: u32 = 1;

impl TasProject {
    pub fn sync_identity_sha256(&self) -> Result<TasDigest> {
        self.validate()?;
        self.sync_identity_sha256_from_validated()
    }

    pub(crate) fn sync_identity_sha256_from_validated(&self) -> Result<TasDigest> {
        let identity = serde_json::to_vec(&self.canonical_identity())?;
        let replay_start = encode_replay_start_metadata(&self.replay_start)?;
        let mut hash = Sha256::new();
        hash.update(b"ZTAS-SYNC-1\0");
        hash.update((identity.len() as u64).to_le_bytes());
        hash.update(identity);
        hash.update((replay_start.len() as u64).to_le_bytes());
        hash.update(replay_start);
        Ok(TasDigest(hash.finalize().into()))
    }

    pub fn branch_movie_sha256(&self, branch_id: &str) -> Result<TasDigest> {
        self.validate()?;
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        self.branch_movie_sha256_from_validated(branch)
    }

    pub(super) fn branch_movie_sha256_from_validated(
        &self,
        branch: &super::model::TasBranch,
    ) -> Result<TasDigest> {
        hash_complete_branch(self.sync_identity_sha256_from_validated()?, branch)
    }

    pub fn branch_prefix_sha256(&self, branch_id: &str, cursor: u64) -> Result<TasDigest> {
        self.validate()?;
        self.branch_prefix_sha256_from_validated(branch_id, cursor)
    }

    pub(crate) fn branch_prefix_sha256_from_validated(
        &self,
        branch_id: &str,
        cursor: u64,
    ) -> Result<TasDigest> {
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        if cursor > branch.frame_count {
            bail!("TAS cursor is past branch end");
        }
        hash_branch(
            self.sync_identity_sha256_from_validated()?,
            branch,
            cursor,
            false,
        )
    }

    pub(crate) fn branch_prefix_sha256_many_from_validated(
        &self,
        branch_id: &str,
        cursors: &[u64],
    ) -> Result<Vec<TasDigest>> {
        let branch = self
            .branch(branch_id)
            .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
        if cursors.iter().any(|&cursor| cursor > branch.frame_count) {
            bail!("TAS cursor is past branch end");
        }
        if cursors.is_empty() {
            return Ok(Vec::new());
        }
        let sync_identity = self.sync_identity_sha256_from_validated()?;
        cursors
            .iter()
            .map(|&cursor| hash_branch(sync_identity, branch, cursor, false))
            .collect()
    }

    pub fn seek_cache_identity(
        &self,
        branch_id: &str,
        cursor: u64,
    ) -> Result<TasSeekCacheIdentity> {
        Ok(TasSeekCacheIdentity {
            cache_format_version: CACHE_FORMAT_VERSION,
            state_format_compatibility_id: self.identity.state_format_compatibility_id.clone(),
            sync_identity_sha256: self.sync_identity_sha256()?,
            branch_prefix_sha256: self.branch_prefix_sha256(branch_id, cursor)?,
            cursor,
        })
    }
}

fn hash_complete_branch(
    sync_identity: TasDigest,
    branch: &super::model::TasBranch,
) -> Result<TasDigest> {
    let input_bytes = serde_json::to_vec(&branch.input_spans)?;
    let event_bytes = encode_canonical_replay_event_stream(&branch.events)?;
    hash_branch_bytes(
        sync_identity,
        branch.frame_count,
        &input_bytes,
        &event_bytes,
    )
}

pub(super) fn hash_branch(
    sync_identity: TasDigest,
    branch: &super::model::TasBranch,
    cursor: u64,
    include_events_at_cursor: bool,
) -> Result<TasDigest> {
    let spans = branch
        .input_spans
        .iter()
        .filter_map(|span| clip_span(*span, cursor))
        .collect::<Vec<_>>();
    let events = branch
        .events
        .iter()
        .filter(|event| {
            event.frame() < cursor || (include_events_at_cursor && event.frame() == cursor)
        })
        .cloned()
        .collect::<Vec<_>>();
    let input_bytes = serde_json::to_vec(&spans)?;
    let event_bytes = encode_replay_event_stream(&events)?;

    hash_branch_bytes(sync_identity, cursor, &input_bytes, &event_bytes)
}

fn hash_branch_bytes(
    sync_identity: TasDigest,
    cursor: u64,
    input_bytes: &[u8],
    event_bytes: &[u8],
) -> Result<TasDigest> {
    let mut hash = Sha256::new();
    hash.update(b"ZTAS-BRANCH-1\0");
    hash.update(sync_identity.0);
    hash.update(cursor.to_le_bytes());
    hash.update((input_bytes.len() as u64).to_le_bytes());
    hash.update(input_bytes);
    hash.update((event_bytes.len() as u64).to_le_bytes());
    hash.update(event_bytes);
    Ok(TasDigest(hash.finalize().into()))
}

fn clip_span(span: TasInputSpan, cursor: u64) -> Option<TasInputSpan> {
    if span.start >= cursor {
        return None;
    }
    Some(TasInputSpan {
        length: span.length.min(cursor - span.start),
        ..span
    })
}

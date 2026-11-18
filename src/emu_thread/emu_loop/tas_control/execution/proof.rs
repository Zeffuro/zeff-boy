use super::*;

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_predecessor_window(
    request: &TasExecutionRequest,
) -> Result<(), Rejected> {
    let Some(window) = &request.predecessor_window else {
        return Ok(());
    };
    if window.source_proofs.len() < 2 || window.source_proofs.len() > 16 {
        return Err(Rejected::InvalidCacheProof);
    }
    let target = request.cache_proof.target_cursor;
    let input_len =
        u64::try_from(window.input_frames.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    let input_end_cursor = window
        .input_start_cursor
        .checked_add(input_len)
        .ok_or(Rejected::InvalidCacheProof)?;
    if window.input_start_cursor == 0
        || window.input_start_cursor >= target
        || input_len != (target - window.input_start_cursor).min(MAX_EDITOR_SEEK_EXECUTION_FRAMES)
    {
        return Err(Rejected::InvalidCacheProof);
    }
    if window.source_proofs.first() != Some(&request.cache_proof)
        || window.source_proofs.last().map(|proof| proof.target_cursor)
            != Some(window.input_start_cursor)
    {
        return Err(Rejected::InvalidCacheProof);
    }
    let mut previous_cursor = None;
    for proof in &window.source_proofs {
        if proof.sync_identity_sha256 != request.cache_proof.sync_identity_sha256
            || proof.target_cursor < window.input_start_cursor
            || (proof.target_cursor > input_end_cursor && proof.target_cursor != target)
            || previous_cursor.is_some_and(|previous| proof.target_cursor >= previous)
        {
            return Err(Rejected::InvalidCacheProof);
        }
        previous_cursor = Some(proof.target_cursor);
    }
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_intermediate_cache_proofs(
    request: &TasExecutionRequest,
) -> Result<(), Rejected> {
    if request
        .intermediate_cache_proofs
        .iter()
        .map(|proof| proof.target_cursor)
        .ne(tas_intermediate_cache_cursors(
            request.cache_proof.target_cursor,
        ))
    {
        return Err(Rejected::InvalidCacheProof);
    }
    let mut previous = 0;
    for proof in &request.intermediate_cache_proofs {
        if proof.sync_identity_sha256 != request.cache_proof.sync_identity_sha256
            || proof.target_cursor <= previous
            || proof.target_cursor >= request.cache_proof.target_cursor
        {
            return Err(Rejected::InvalidCacheProof);
        }
        previous = proof.target_cursor;
    }
    Ok(())
}

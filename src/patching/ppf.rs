use anyhow::{Context, ensure};

const DESCRIPTION_END: usize = 56;
const BLOCK_CHECK_LEN: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PpfPlanLimits {
    max_patch_bytes: usize,
    max_records: usize,
    max_replacement_bytes: usize,
}

impl PpfPlanLimits {
    pub(crate) const UNBOUNDED: Self = Self::new(usize::MAX, usize::MAX, usize::MAX);

    pub(crate) const fn new(
        max_patch_bytes: usize,
        max_records: usize,
        max_replacement_bytes: usize,
    ) -> Self {
        Self {
            max_patch_bytes,
            max_records,
            max_replacement_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PpfPlanFallback {
    PatchBytes { bytes: usize, limit: usize },
    Records { records: usize, limit: usize },
    ReplacementBytes { bytes: usize, limit: usize },
}

#[derive(Clone, Copy)]
pub(crate) struct PpfWrite<'a> {
    pub(crate) offset: u64,
    pub(crate) bytes: &'a [u8],
}

pub(crate) struct PpfPatchPlan<'a> {
    writes: Vec<PpfWrite<'a>>,
}

impl<'a> PpfPatchPlan<'a> {
    pub(crate) fn writes(&self) -> &[PpfWrite<'a>] {
        &self.writes
    }
}

pub(crate) enum PpfPlanOutcome<'a> {
    Planned(PpfPatchPlan<'a>),
    Fallback(PpfPlanFallback),
}

pub(crate) fn apply_ppf_patch(target: &mut Vec<u8>, patch: &[u8]) -> anyhow::Result<()> {
    apply_ppf_patch_segments(std::slice::from_mut(target), patch)
}

pub(crate) fn ppf_has_source_validation(patch: &[u8]) -> bool {
    match patch.get(..5) {
        Some(b"PPF20") => true,
        Some(b"PPF30") => patch.get(57) == Some(&1),
        _ => false,
    }
}

pub(crate) fn apply_ppf_patch_segments(
    targets: &mut [Vec<u8>],
    patch: &[u8],
) -> anyhow::Result<()> {
    let total_len = targets.iter().try_fold(0_u64, |total, target| {
        total.checked_add(target.len() as u64)
    });
    let total_len = total_len.context("PPF target is too large")?;
    let outcome = plan_ppf_patch(
        patch,
        total_len,
        PpfPlanLimits::UNBOUNDED,
        &mut |offset, output| read_segments(targets, offset, output),
    )?;
    let plan = match outcome {
        PpfPlanOutcome::Planned(plan) => plan,
        PpfPlanOutcome::Fallback(reason) => {
            anyhow::bail!("unbounded PPF planning unexpectedly fell back: {reason:?}")
        }
    };

    for record in plan.writes() {
        write_segments(targets, record.offset, record.bytes);
    }
    Ok(())
}

pub(crate) fn plan_ppf_patch<'a>(
    patch: &'a [u8],
    target_len: u64,
    limits: PpfPlanLimits,
    read_source: &mut dyn FnMut(u64, &mut [u8]) -> anyhow::Result<()>,
) -> anyhow::Result<PpfPlanOutcome<'a>> {
    if patch.len() > limits.max_patch_bytes {
        return Ok(PpfPlanOutcome::Fallback(PpfPlanFallback::PatchBytes {
            bytes: patch.len(),
            limit: limits.max_patch_bytes,
        }));
    }
    ensure!(patch.len() >= DESCRIPTION_END, "PPF header truncated");
    let (offset_bytes, mut cursor, record_end, undo) = match &patch[..5] {
        b"PPF10" => {
            ensure!(patch[5] == 0, "invalid PPF1 encoding");
            (4, DESCRIPTION_END, patch.len(), false)
        }
        b"PPF20" => {
            ensure!(patch[5] == 1, "invalid PPF2 encoding");
            ensure!(patch.len() >= 1084, "PPF2 header truncated");
            let expected_len = u32::from_le_bytes(patch[56..60].try_into().unwrap()) as u64;
            ensure!(
                expected_len == target_len,
                "PPF2 target size mismatch: expected {expected_len}, got {target_len}"
            );
            verify_block(target_len, 0x9320, &patch[60..1084], read_source)?;
            (4, 1084, record_end(patch, 4, 38)?, false)
        }
        b"PPF30" => {
            ensure!(patch.len() >= 60, "PPF3 header truncated");
            ensure!(patch[5] == 2, "invalid PPF3 encoding");
            ensure!(patch[56] <= 1, "invalid PPF3 image type");
            ensure!(patch[57] <= 1, "invalid PPF3 block-check flag");
            ensure!(patch[58] <= 1, "invalid PPF3 undo flag");
            ensure!(patch[59] == 0, "invalid PPF3 reserved byte");
            let has_block_check = patch[57] != 0;
            let header_len = if has_block_check { 1084 } else { 60 };
            ensure!(patch.len() >= header_len, "PPF3 header truncated");
            if has_block_check {
                let offset = if patch[56] == 0 { 0x9320 } else { 0x80a0 };
                verify_block(target_len, offset, &patch[60..1084], read_source)?;
            }
            (8, header_len, record_end(patch, 2, 36)?, patch[58] != 0)
        }
        _ => anyhow::bail!("unsupported PPF patch version"),
    };

    ensure!(record_end >= cursor, "PPF record area is malformed");
    let mut records = Vec::new();
    let mut replacement_bytes = 0_usize;
    while cursor < record_end {
        let header_end = cursor
            .checked_add(offset_bytes + 1)
            .context("PPF record header overflow")?;
        ensure!(header_end <= record_end, "PPF record header truncated");
        let offset = if offset_bytes == 4 {
            u32::from_le_bytes(patch[cursor..cursor + 4].try_into().unwrap()) as u64
        } else {
            u64::from_le_bytes(patch[cursor..cursor + 8].try_into().unwrap())
        };
        let len = patch[header_end - 1] as usize;
        let data_start = header_end;
        let data_end = data_start
            .checked_add(len)
            .context("PPF record length overflow")?;
        let next = data_end
            .checked_add(if undo { len } else { 0 })
            .context("PPF undo record length overflow")?;
        ensure!(next <= record_end, "PPF record data truncated");
        let target_end = offset
            .checked_add(len as u64)
            .context("PPF target offset overflow")?;
        ensure!(
            target_end <= target_len,
            "PPF record ends outside target at {target_end:#x} (target length {target_len:#x})"
        );
        let record_count = records
            .len()
            .checked_add(1)
            .context("PPF record count overflow")?;
        if record_count > limits.max_records {
            return Ok(PpfPlanOutcome::Fallback(PpfPlanFallback::Records {
                records: record_count,
                limit: limits.max_records,
            }));
        }
        replacement_bytes = replacement_bytes
            .checked_add(len)
            .context("PPF replacement length overflow")?;
        if replacement_bytes > limits.max_replacement_bytes {
            return Ok(PpfPlanOutcome::Fallback(
                PpfPlanFallback::ReplacementBytes {
                    bytes: replacement_bytes,
                    limit: limits.max_replacement_bytes,
                },
            ));
        }
        records.push(PpfWrite {
            offset,
            bytes: &patch[data_start..data_end],
        });
        cursor = next;
    }
    ensure!(cursor == record_end, "PPF record area is malformed");
    Ok(PpfPlanOutcome::Planned(PpfPatchPlan { writes: records }))
}

fn record_end(patch: &[u8], length_bytes: usize, trailer_overhead: usize) -> anyhow::Result<usize> {
    if patch.len() < length_bytes + 4
        || &patch[patch.len() - length_bytes - 4..patch.len() - length_bytes] != b".DIZ"
    {
        return Ok(patch.len());
    }
    let id_len = match length_bytes {
        2 => u16::from_le_bytes(patch[patch.len() - 2..].try_into().unwrap()) as usize,
        4 => u32::from_le_bytes(patch[patch.len() - 4..].try_into().unwrap()) as usize,
        _ => unreachable!(),
    };
    patch
        .len()
        .checked_sub(id_len)
        .and_then(|end| end.checked_sub(trailer_overhead))
        .context("PPF file ID trailer is malformed")
}

fn verify_block(
    target_len: u64,
    offset: u64,
    expected: &[u8],
    read_source: &mut dyn FnMut(u64, &mut [u8]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    ensure!(
        expected.len() == BLOCK_CHECK_LEN,
        "PPF block check truncated"
    );
    ensure!(
        offset + BLOCK_CHECK_LEN as u64 <= target_len,
        "PPF target is too short for its block check"
    );
    let mut actual = [0; BLOCK_CHECK_LEN];
    read_source(offset, &mut actual)?;
    ensure!(actual == expected, "PPF source block check failed");
    Ok(())
}

fn read_segments(
    targets: &[Vec<u8>],
    mut offset: u64,
    mut output: &mut [u8],
) -> anyhow::Result<()> {
    for target in targets {
        if offset >= target.len() as u64 {
            offset -= target.len() as u64;
            continue;
        }
        let start = offset as usize;
        let take = output.len().min(target.len() - start);
        let (head, tail) = output.split_at_mut(take);
        head.copy_from_slice(&target[start..start + take]);
        output = tail;
        offset = 0;
        if output.is_empty() {
            break;
        }
    }
    ensure!(output.is_empty(), "PPF source read ends outside target");
    Ok(())
}

fn write_segments(targets: &mut [Vec<u8>], mut offset: u64, mut bytes: &[u8]) {
    for target in targets {
        if offset >= target.len() as u64 {
            offset -= target.len() as u64;
            continue;
        }
        let start = offset as usize;
        let take = bytes.len().min(target.len() - start);
        target[start..start + take].copy_from_slice(&bytes[..take]);
        bytes = &bytes[take..];
        offset = 0;
        if bytes.is_empty() {
            break;
        }
    }
    debug_assert!(bytes.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ppf3(records: &[(u64, &[u8])], block: Option<&[u8]>, undo: bool) -> Vec<u8> {
        let mut patch = b"PPF30\x02".to_vec();
        patch.resize(56, 0);
        patch.extend_from_slice(&[0, u8::from(block.is_some()), u8::from(undo), 0]);
        if let Some(block) = block {
            assert_eq!(block.len(), BLOCK_CHECK_LEN);
            patch.extend_from_slice(block);
        }
        for (offset, bytes) in records {
            patch.extend_from_slice(&offset.to_le_bytes());
            patch.push(bytes.len() as u8);
            patch.extend_from_slice(bytes);
            if undo {
                patch.resize(patch.len() + bytes.len(), 0xcc);
            }
        }
        patch
    }

    #[test]
    fn ppf3_applies_across_segment_boundaries_and_skips_undo_data() {
        let mut targets = vec![vec![0; 4], vec![0; 4]];
        let patch = ppf3(&[(3, &[1, 2, 3])], None, true);
        apply_ppf_patch_segments(&mut targets, &patch).unwrap();
        assert_eq!(targets, vec![vec![0, 0, 0, 1], vec![2, 3, 0, 0]]);
    }

    #[test]
    fn ppf3_block_check_is_strict_and_transactional() {
        let mut target = vec![0; 0x9320 + BLOCK_CHECK_LEN];
        let mut expected = vec![0; BLOCK_CHECK_LEN];
        expected[4] = 7;
        let patch = ppf3(&[(0, &[9])], Some(&expected), false);
        let original = target.clone();
        assert!(apply_ppf_patch(&mut target, &patch).is_err());
        assert_eq!(target, original);
    }

    #[test]
    fn ppf2_checks_target_size_and_source_block() {
        let mut target = vec![0; 0x9320 + BLOCK_CHECK_LEN];
        target[0x9324] = 7;
        let mut patch = b"PPF20\x01".to_vec();
        patch.resize(56, 0);
        patch.extend_from_slice(&(target.len() as u32).to_le_bytes());
        patch.extend_from_slice(&target[0x9320..0x9320 + BLOCK_CHECK_LEN]);
        patch.extend_from_slice(&4_u32.to_le_bytes());
        patch.push(2);
        patch.extend_from_slice(&[8, 9]);
        apply_ppf_patch(&mut target, &patch).unwrap();
        assert_eq!(&target[4..6], &[8, 9]);
    }

    #[test]
    fn ppf1_rejects_out_of_range_record_without_mutating() {
        let mut target = vec![0; 4];
        let mut patch = b"PPF10\0".to_vec();
        patch.resize(56, 0);
        patch.extend_from_slice(&3_u32.to_le_bytes());
        patch.push(2);
        patch.extend_from_slice(&[1, 2]);
        let original = target.clone();
        assert!(apply_ppf_patch(&mut target, &patch).is_err());
        assert_eq!(target, original);
    }

    #[test]
    fn planner_returns_typed_fallbacks() {
        let mut patch = b"PPF10\0".to_vec();
        patch.resize(56, 0);
        for offset in [0_u32, 4] {
            patch.extend_from_slice(&offset.to_le_bytes());
            patch.push(2);
            patch.extend_from_slice(&[1, 2]);
        }
        let mut unread = |_: u64, _: &mut [u8]| -> anyhow::Result<()> {
            panic!("PPF1 must not read the source")
        };
        let outcome = plan_ppf_patch(
            &patch,
            8,
            PpfPlanLimits::new(patch.len() - 1, usize::MAX, usize::MAX),
            &mut unread,
        )
        .unwrap();
        assert!(matches!(
            outcome,
            PpfPlanOutcome::Fallback(PpfPlanFallback::PatchBytes { .. })
        ));

        let outcome = plan_ppf_patch(
            &patch,
            8,
            PpfPlanLimits::new(patch.len(), 1, usize::MAX),
            &mut unread,
        )
        .unwrap();
        assert!(matches!(
            outcome,
            PpfPlanOutcome::Fallback(PpfPlanFallback::Records {
                records: 2,
                limit: 1
            })
        ));

        let outcome = plan_ppf_patch(
            &patch,
            8,
            PpfPlanLimits::new(patch.len(), usize::MAX, 3),
            &mut unread,
        )
        .unwrap();
        assert!(matches!(
            outcome,
            PpfPlanOutcome::Fallback(PpfPlanFallback::ReplacementBytes { bytes: 4, limit: 3 })
        ));

        let outcome = plan_ppf_patch(
            &patch,
            8,
            PpfPlanLimits::new(patch.len(), 2, 4),
            &mut unread,
        )
        .unwrap();
        let PpfPlanOutcome::Planned(plan) = outcome else {
            panic!("exact limits must be accepted");
        };
        assert_eq!(plan.writes().len(), 2);
    }
}

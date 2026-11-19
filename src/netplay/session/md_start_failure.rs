use super::*;
use std::cell::Cell;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    FirstCapture,
    SecondCapture,
    FrameZero,
    Network,
    AfterNetwork,
    AbortVerification,
}
impl Stage {
    pub(crate) fn triggers_at(self, stage: Self) -> bool {
        self == stage || (self == Self::AbortVerification && stage == Self::FirstCapture)
    }
}
#[derive(Debug)]
pub(crate) struct Failure(pub(crate) Stage);
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MD start failed at {:?}", self.0)
    }
}
impl std::error::Error for Failure {}
thread_local! { static NEXT: Cell<Option<Stage>> = const { Cell::new(None) }; }
pub(crate) fn next(stage: Stage) {
    NEXT.with(|next| next.set(Some(stage)));
}
pub(super) fn at(lease: &Lease, backend: &mut EmuBackend, stage: Stage) -> Result<()> {
    if !lease.is_md() {
        return Ok(());
    }
    let armed = if let EmuBackend::Md(md) = backend {
        md.take_start_failure_for_test(stage)
    } else {
        None
    };
    let armed = armed.or_else(|| {
        NEXT.with(|next| {
            let target = next.get().filter(|target| target.triggers_at(stage))?;
            next.set(None);
            Some(target)
        })
    });
    if let Some(stage) = armed {
        return Err(Failure(stage).into());
    }
    Ok(())
}

pub(super) fn abort_checkpoint(error: &anyhow::Error, raw: &[u8]) -> Option<Vec<u8>> {
    matches!(
        error.downcast_ref::<Failure>(),
        Some(Failure(Stage::AbortVerification))
    )
    .then(|| raw[..raw.len().min(1)].to_vec())
}

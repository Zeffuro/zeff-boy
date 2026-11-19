use zeff_md_core::emulator::DebugPortWrite;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MdTimedPortRequest {
    pub(crate) id: u64,
    pub(crate) writes: Vec<DebugPortWrite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MdTimedPortOutcome {
    pub(crate) writes: usize,
    pub(crate) cpu_clocks: u64,
    pub(crate) master_ticks: u64,
    pub(crate) instructions: u64,
    pub(crate) fields_crossed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MdTimedPortFailureKind {
    Context,
    Conflict,
    Lifecycle,
    Port,
    Execution,
    Checkpoint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MdTimedPortFailure {
    pub(crate) kind: MdTimedPortFailureKind,
    pub(crate) index: Option<usize>,
    pub(crate) reason: String,
}

impl MdTimedPortFailure {
    pub(crate) fn new(kind: MdTimedPortFailureKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            index: None,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MdTimedPortReply {
    pub(crate) id: u64,
    pub(crate) result: Result<MdTimedPortOutcome, MdTimedPortFailure>,
    pub(crate) queued_pcm_samples_before: Option<usize>,
    pub(crate) delivered_pcm_samples: usize,
}

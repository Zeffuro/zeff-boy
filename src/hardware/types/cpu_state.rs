#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub(crate) enum IMEState {
    Enabled,
    Disabled,
    PendingEnable,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub(crate) enum CPUState {
    Running,
    Halted,
    Stopped,
    InterruptHandling,
    Reset,
    Suspended,
}

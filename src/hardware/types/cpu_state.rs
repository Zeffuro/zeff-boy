#[derive(PartialEq, Eq)]
pub(crate) enum IMEState {
    Enabled,
    Disabled,
    PendingEnable,
}

#[derive(PartialEq, Eq)]
pub(crate) enum CPUState {
    Running,
    Halted,
    Stopped,
    InterruptHandling,
    Reset,
    Suspended,
}

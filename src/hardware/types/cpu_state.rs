#[derive(PartialEq, Eq, Copy, Clone)]
pub(crate) enum IMEState {
    Enabled,
    Disabled,
    PendingEnable,
}

#[derive(PartialEq, Eq, Copy, Clone)]
pub(crate) enum CPUState {
    Running,
    Halted,
    Stopped,
    InterruptHandling,
    Reset,
    Suspended,
}

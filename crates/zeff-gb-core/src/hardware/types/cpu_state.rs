#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum IMEState {
    Enabled,
    Disabled,
    PendingEnable,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum CPUState {
    Running,
    Halted,
    Stopped,
    InterruptHandling,
    Reset,
    Suspended,
}

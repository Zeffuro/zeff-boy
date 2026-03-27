#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ImeState {
    Enabled,
    Disabled,
    PendingEnable,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum CpuState {
    Running,
    Halted,
    Stopped,
    InterruptHandling,
    Reset,
    Suspended,
}

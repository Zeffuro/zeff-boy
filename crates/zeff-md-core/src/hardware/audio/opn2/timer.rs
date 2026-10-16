mod a;
mod b;
mod state;
pub(super) use a::TimerA;
pub(super) use b::TimerB;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LegacyTimer {
    pub reload: u16,
    remaining: u64,
    running: bool,
}

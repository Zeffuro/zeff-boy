#![forbid(unsafe_code)]

pub mod address;
pub mod audio_trace;
pub mod cheats;
pub mod cpu;
pub mod debug;
#[cfg(feature = "host-clock")]
pub mod host_clock;
pub mod media;
pub mod memory;
pub mod replay;
pub mod rewind;
pub mod save_ram;
pub mod save_state;
pub mod system;
pub mod time;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateRestoreOutcome {
    Exact,
    BestEffortBess,
    BestEffortPortable,
}

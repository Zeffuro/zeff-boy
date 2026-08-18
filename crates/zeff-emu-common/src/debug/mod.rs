mod address_controller;
mod controller;
mod instruction_trace;
mod opcode_log;
mod types;

#[cfg(test)]
mod tests;

pub use address_controller::AddressDebugController;
pub use controller::DebugController;
pub use instruction_trace::{
    InstructionTraceRecord, InstructionTraceStore, MAX_TRACE_CAPACITY, MAX_TRACE_INSTRUCTION_BYTES,
    MAX_TRACE_REGISTER_DELTAS, MAX_TRACE_WRITES, MIN_TRACE_CAPACITY, RegisterDelta, TraceEntry,
    TraceExecMode, TraceWrite, TraceWriteKind, TraceWriteWidth,
};
pub use opcode_log::OpcodeLog;
pub use types::{
    AddressWatchHit, AddressWatchpoint, BreakpointHitCondition, DebugEvent, WatchHit, WatchType,
    Watchpoint,
};

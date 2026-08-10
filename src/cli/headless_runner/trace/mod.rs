mod gba;
mod nes;
mod ws;

pub(super) use gba::{
    format_gba_bus_trace_line, format_gba_op_line, format_gba_op_tail_line, gba_bad_state_reason,
    should_trace_gba_bus_event, should_trace_gba_op,
};
pub(super) use nes::{
    format_nes_bus_trace_line, format_nes_op_line, nes_op_extra, should_trace_nes_bus_event,
    should_trace_nes_op,
};
pub(super) use ws::{
    format_ws_bus_trace_line, format_ws_op_line, format_ws_op_tail_line, should_trace_ws_bus_event,
    should_trace_ws_op,
};

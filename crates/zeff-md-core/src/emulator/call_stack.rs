mod rollback_memory;
use super::Emulator;
use std::collections::VecDeque;
use zeff_m68k::timing::{ControlFlowBatch, ControlFlowEntryKind, ControlFlowEvent, ControlStack};

mod codec;

pub(super) const MAX_CALL_STACK_BYTES: usize = 16 * 1024;
const MAX_FRAMES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallStackFrame {
    pub kind: ControlFlowEntryKind,
    pub source_pc: u32,
    pub target_pc: u32,
    pub return_pc: u32,
    pub stack: ControlStack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStackStatus {
    Disabled,
    Unknown,
    Observed { truncated: bool },
}

#[derive(Clone, Debug)]
struct Recording {
    frames: VecDeque<CallStackFrame>,
    unknown: bool,
    truncated: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Tracker {
    recording: Option<Box<Recording>>,
}

// Captured debugger history is excluded from machine hardware equality.
impl PartialEq for Tracker {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for Tracker {}

impl Tracker {
    fn enabled(&self) -> bool {
        self.recording.is_some()
    }

    fn set_enabled(&mut self, enabled: bool) {
        if self.enabled() != enabled {
            self.recording = enabled.then(|| {
                Box::new(Recording {
                    frames: VecDeque::new(),
                    unknown: true,
                    truncated: false,
                })
            });
        }
    }

    fn status(&self) -> CallStackStatus {
        match self.recording.as_deref() {
            None => CallStackStatus::Disabled,
            Some(recording) if recording.unknown => CallStackStatus::Unknown,
            Some(recording) => CallStackStatus::Observed {
                truncated: recording.truncated,
            },
        }
    }

    fn frames(&self) -> impl DoubleEndedIterator<Item = &CallStackFrame> {
        self.recording
            .iter()
            .flat_map(|recording| recording.frames.iter())
    }

    fn invalidate(&mut self) {
        if let Some(recording) = &mut self.recording {
            recording.frames.clear();
            recording.unknown = true;
            recording.truncated = false;
        }
    }

    fn apply(&mut self, batch: ControlFlowBatch) {
        if batch.overflowed() {
            self.invalidate();
            return;
        }
        let Some(recording) = &mut self.recording else {
            return;
        };
        for &event in batch.events() {
            match event {
                ControlFlowEvent::Entry {
                    kind,
                    source_pc,
                    target_pc,
                    return_pc,
                    stack,
                } => {
                    if recording.frames.len() == MAX_FRAMES {
                        recording.frames.pop_front();
                        recording.truncated = true;
                    }
                    recording.frames.push_back(CallStackFrame {
                        kind,
                        source_pc,
                        target_pc,
                        return_pc,
                        stack,
                    });
                    recording.unknown = false;
                }
                ControlFlowEvent::Return {
                    target_pc, stack, ..
                } => {
                    if let Some(index) = recording.frames.iter().rposition(|frame| {
                        frame.return_pc == target_pc
                            && frame.stack.bank == stack.bank
                            && frame.stack.pc_slot == stack.pc_slot
                            && frame.stack.before == stack.after
                    }) {
                        recording.frames.truncate(index);
                    } else {
                        recording.frames.clear();
                        recording.unknown = true;
                        recording.truncated = false;
                    }
                }
            }
        }
    }
}

impl Emulator {
    pub fn set_call_stack_recording(&mut self, enabled: bool) {
        self.call_stack.set_enabled(enabled);
        self.cpu.set_control_flow_recording(enabled);
    }

    pub fn call_stack_status(&self) -> CallStackStatus {
        self.call_stack.status()
    }

    pub fn call_stack(&self) -> impl DoubleEndedIterator<Item = &CallStackFrame> {
        self.call_stack.frames()
    }

    pub(super) fn reset_call_stack(&mut self) {
        self.call_stack.invalidate();
        self.cpu
            .set_control_flow_recording(self.call_stack.enabled());
        self.cpu.clear_control_flow_events();
    }

    pub(super) fn begin_call_stack_operation(&mut self) {
        if self.call_stack.enabled() {
            self.cpu.clear_control_flow_events();
        }
    }

    pub(super) fn finish_call_stack_operation(&mut self, success: bool) {
        if self.call_stack.enabled() {
            let batch = self.cpu.drain_control_flow_events();
            if success {
                self.call_stack.apply(batch);
            } else {
                self.call_stack.invalidate();
            }
        }
    }
}

#[cfg(test)]
mod tests;

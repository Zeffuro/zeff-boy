use super::PrefetchCpu;
use crate::Registers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlStackBank {
    User,
    Supervisor,
}

impl ControlStackBank {
    pub(super) fn active(registers: &Registers) -> Self {
        if registers.supervisor() {
            Self::Supervisor
        } else {
            Self::User
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlStack {
    pub bank: ControlStackBank,
    pub before: u32,
    pub after: u32,
    pub pc_slot: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlFlowEntryKind {
    Jsr,
    Bsr,
    Debugger,
    Exception(u8),
    Interrupt { level: u8, vector: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlFlowReturnKind {
    Rts,
    Rtr,
    Rte,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlFlowEvent {
    Entry {
        kind: ControlFlowEntryKind,
        source_pc: u32,
        target_pc: u32,
        return_pc: u32,
        stack: ControlStack,
    },
    Return {
        kind: ControlFlowReturnKind,
        source_pc: u32,
        target_pc: u32,
        supervisor_after: bool,
        stack: ControlStack,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControlFlowBatch {
    events: [Option<ControlFlowEvent>; 2],
    overflowed: bool,
}

impl ControlFlowBatch {
    pub fn events(&self) -> impl Iterator<Item = &ControlFlowEvent> {
        self.events.iter().flatten()
    }

    pub fn overflowed(&self) -> bool {
        self.overflowed
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Mailbox {
    enabled: bool,
    pending: ControlFlowBatch,
}

// Debug metadata is excluded from hardware equality.
impl PartialEq for Mailbox {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for Mailbox {}

impl Mailbox {
    pub(super) fn has_diagnostics(&self) -> bool {
        self.enabled || self.pending != ControlFlowBatch::default()
    }

    fn push(&mut self, event: ControlFlowEvent) {
        if self.enabled {
            if let Some(slot) = self.pending.events.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(event);
            } else {
                self.pending.overflowed = true;
            }
        }
    }
}

impl PrefetchCpu {
    pub fn set_control_flow_recording(&mut self, enabled: bool) {
        if self.control_events.enabled != enabled {
            self.clear_control_flow_events();
            self.control_events.enabled = enabled;
        }
    }

    pub fn control_flow_recording(&self) -> bool {
        self.control_events.enabled
    }

    pub fn clear_control_flow_events(&mut self) {
        self.control_events.pending = ControlFlowBatch::default();
    }

    /// Committed prefix facts only; the owner invalidates history on CPU or later machine failure.
    pub fn drain_control_flow_events(&mut self) -> ControlFlowBatch {
        std::mem::take(&mut self.control_events.pending)
    }

    pub(super) fn control_stack(
        &self,
        bank: ControlStackBank,
        before: u32,
        pc_slot: u32,
    ) -> ControlStack {
        ControlStack {
            bank,
            before,
            after: match bank {
                ControlStackBank::User => self.registers.usp,
                ControlStackBank::Supervisor => self.registers.ssp,
            },
            pc_slot,
        }
    }

    pub(super) fn record_control_entry(
        &mut self,
        kind: ControlFlowEntryKind,
        source_pc: u32,
        return_pc: u32,
        stack: ControlStack,
    ) {
        if self.control_events.enabled {
            let event = ControlFlowEvent::Entry {
                kind,
                source_pc,
                target_pc: self.registers.pc,
                return_pc,
                stack,
            };
            self.control_events.push(event);
        }
    }

    pub(super) fn record_control_return(
        &mut self,
        kind: ControlFlowReturnKind,
        source_pc: u32,
        bank: ControlStackBank,
        before: u32,
        pc_slot: u32,
    ) {
        if self.control_events.enabled {
            let event = ControlFlowEvent::Return {
                kind,
                source_pc,
                target_pc: self.registers.pc,
                supervisor_after: self.registers.supervisor(),
                stack: self.control_stack(bank, before, pc_slot),
            };
            self.control_events.push(event);
        }
    }
}

#[cfg(test)]
mod tests;

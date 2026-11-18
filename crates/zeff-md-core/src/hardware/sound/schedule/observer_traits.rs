use super::*;

impl Clone for PendingSound {
    fn clone(&self) -> Self {
        let mut cloned = Self {
            cpu: self.cpu.clone(),
            memory: self.memory.clone(),
            instruction: self.instruction,
            deadline: self.deadline,
            state: self.state,
            writes: self.writes,
            register_writes: self.register_writes,
            sf001_writes: self.sf001_writes,
            sf002_writes: self.sf002_writes,
            psg_writes: self.psg_writes,
            opn2_writes: self.opn2_writes,
            bank_access: self.bank_access,
            observed_bank_access: self.observed_bank_access,
            any_bank_access: self.any_bank_access,
            start: self.start,
            replay: self.replay,
            trace_origin: self.trace_origin,
            svp_history: self.svp_history.clone(),
            observer: observer::Token::empty(),
        };
        cloned.observer = observer::image(&cloned.memory.ram.1, Some(&self.observer));
        cloned
    }
}

impl PartialEq for PendingSound {
    fn eq(&self, other: &Self) -> bool {
        self.cpu == other.cpu
            && self.memory == other.memory
            && self.instruction == other.instruction
            && self.deadline == other.deadline
            && self.state == other.state
            && self.writes == other.writes
            && self.register_writes == other.register_writes
            && self.sf001_writes == other.sf001_writes
            && self.sf002_writes == other.sf002_writes
            && self.psg_writes == other.psg_writes
            && self.opn2_writes == other.opn2_writes
            && self.bank_access == other.bank_access
            && self.observed_bank_access == other.observed_bank_access
            && self.any_bank_access == other.any_bank_access
            && self.start == other.start
            && self.replay == other.replay
            && self.trace_origin == other.trace_origin
            && self.svp_history == other.svp_history
    }
}

impl Eq for PendingSound {}

impl std::fmt::Debug for PendingSound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingSound")
            .field("cpu", &self.cpu)
            .field("memory", &self.memory)
            .field("instruction", &self.instruction)
            .field("deadline", &self.deadline)
            .field("state", &self.state)
            .field("writes", &self.writes)
            .field("register_writes", &self.register_writes)
            .field("sf001_writes", &self.sf001_writes)
            .field("sf002_writes", &self.sf002_writes)
            .field("psg_writes", &self.psg_writes)
            .field("opn2_writes", &self.opn2_writes)
            .field("bank_access", &self.bank_access)
            .field("observed_bank_access", &self.observed_bank_access)
            .field("any_bank_access", &self.any_bank_access)
            .field("start", &self.start)
            .field("replay", &self.replay)
            .field("trace_origin", &self.trace_origin)
            .field("svp_history", &self.svp_history)
            .finish()
    }
}

#[cfg(test)]
impl PendingSound {
    pub(crate) fn observer_image_id(&self) -> u64 {
        self.observer.identity()
    }
}

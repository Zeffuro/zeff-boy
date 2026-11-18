use super::*;
use zeff_emu_common::debug::InstructionTraceStore;

mod fold;
mod writes;
pub(crate) use fold::Fold;

pub(crate) struct Attachment<'a> {
    journal: &'a mut Journal,
    fold: &'a mut Fold,
    output: &'a mut InstructionTraceStore,
    pending: Option<([Option<Event>; HALF_EVENTS], bool)>,
}
impl<'a> Attachment<'a> {
    pub(crate) fn new(
        journal: &'a mut Journal,
        fold: &'a mut Fold,
        output: &'a mut InstructionTraceStore,
    ) -> Self {
        journal.set_enabled(output.is_enabled());
        fold.set_enabled(output.is_enabled(), output);
        Self {
            journal,
            fold,
            output,
            pending: None,
        }
    }
}
impl PhysicalCapture for Attachment<'_> {
    type Half = Half;
    fn begin_half(&self, half: u64) -> Half {
        self.journal.begin_half(half)
    }
    fn accept_half(&mut self, half: Half) {
        self.pending = Some((half.events, half.overflow));
        self.journal.accept_half(half);
    }
    fn after_commit(&mut self, cpu: &zeff_z80::Cpu) {
        if let Some((events, overflow)) = self.pending.take() {
            self.fold.accept(events, overflow, cpu, self.output);
        }
    }
    fn fault(&mut self) {
        self.journal.fault();
        self.fold.fault(self.output);
    }
    fn position(&self) -> Option<Stamp> {
        self.journal.position()
    }
}

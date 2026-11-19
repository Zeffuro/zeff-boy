use super::MdBackend;
use crate::ui::TraceStreamId;

impl MdBackend {
    pub(crate) fn instruction_trace_stream(&self) -> TraceStreamId {
        self.trace_stream
    }

    pub(in crate::emu_backend::md) fn rotate_trace_stream(&mut self) {
        let stream = if self.trace_stream.is_live() {
            TraceStreamId::allocate()
        } else {
            self.trace_stream
        };
        self.install_trace_stream(stream);
    }

    fn install_trace_stream(&mut self, stream: TraceStreamId) {
        self.trace_stream = stream;
        if !stream.is_live() {
            self.debugger.trace.set_enabled(false);
            self.debugger.trace.clear();
        }
    }

    pub(in crate::emu_backend::md) fn clear_instruction_trace(&mut self) {
        self.debugger.trace.clear();
        self.rotate_trace_stream();
    }

    #[cfg(test)]
    pub(crate) fn exhaust_trace_stream_for_test(&mut self) {
        self.install_trace_stream(TraceStreamId::exhausted_for_test());
    }
}

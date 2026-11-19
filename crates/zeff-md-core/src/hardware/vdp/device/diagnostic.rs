use super::{State, Vdp};
use crate::diagnostic::{RegisterPending, RegisterRejection, RejectionSite, VdpContext};

impl State {
    pub(in crate::hardware::vdp::device) fn diagnostic_context(&self) -> VdpContext {
        VdpContext {
            ticks: self.now().get(),
            register_ticks: self.registers.now().get(),
            publication_epoch: self.registers.published_state().0.get(),
            registers: *self.registers.registers(),
            pending: self.registers.pending().map(|write| RegisterPending {
                index: write.index,
                value: write.value,
                publish_at: write.publish_at.get(),
            }),
            read_phase: self.read_phase,
            fifo_len: self.fifo.len(),
            control_pending_word: self.control.pending_word(),
            dma_active: self.dma.is_some(),
            timing_history: self.interrupts.has_timing_history(),
            normal_interlace_history: self.interrupts.has_normal_interlace_history(),
        }
    }

    pub(in crate::hardware::vdp::device) fn record_rejection(
        &self,
        site: RejectionSite,
        index: u8,
        value: u8,
    ) {
        crate::diagnostic::ControlScope::reject(RegisterRejection {
            site,
            index,
            value,
            attempted_publish_at: None,
            context: self.diagnostic_context(),
        });
    }

    pub(in crate::hardware::vdp::device) fn record_clock_rejection(
        &self,
        site: RejectionSite,
        write: super::VdpRegisterWrite,
    ) {
        crate::diagnostic::ControlScope::reject(RegisterRejection {
            site,
            index: write.index,
            value: write.value,
            attempted_publish_at: Some(write.publish_at.get()),
            context: self.diagnostic_context(),
        });
    }
}

impl Vdp {
    pub(crate) fn diagnostic_context(&self) -> VdpContext {
        self.assert_legacy();
        self.state.diagnostic_context()
    }
}

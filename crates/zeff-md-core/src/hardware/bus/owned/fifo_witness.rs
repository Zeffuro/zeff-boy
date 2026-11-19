use super::*;
use crate::hardware::vdp::owner::BoardView;
use zeff_m68k::timing::{DebugDataWrite, PrefetchCpu};

#[derive(Clone, Debug, Default)]
pub(crate) struct Witness {
    preview: Option<(BoardView, Option<main_bus::Pending>)>,
    sampled: Option<(BoardView, Option<main_bus::Pending>)>,
    pub(crate) full_wait: Option<(main_bus::Pending, u64, u8, u64, u8)>,
    receipt: Option<main_bus::Receipt>,
    pub(crate) dma_pressure: Option<(u64, u8, u32)>,
}

impl Witness {
    pub(super) fn resolve(
        &mut self,
        view: BoardView,
        before: Option<main_bus::Snapshot>,
        after: Option<main_bus::Snapshot>,
    ) {
        self.preview = Some((view, before.and_then(|state| state.pending)));
        if let Some(receipt) = after.and_then(|state| state.completed) {
            self.receipt = Some(receipt);
        }
        if let (Some((drive, Some(source))), Some(before), Some(after)) =
            (self.sampled, before, after)
            && let (Some(pending), Some(next)) = (before.pending, after.pending)
            && drive.fifo_full
            && !drive.drives.levels.acknowledge
            && source.request == pending.request
            && source.start_half == pending.start_half
            && next.request == pending.request
            && next.start_half == pending.start_half
            && pending.request.access.logical_address == 0xc00000
            && pending.request.access.bus_address == 0xc00000
            && pending.request.access.kind == AccessKind::Data
            && pending.request.access.lanes == ByteLanes::Both
            && matches!(pending.request.operation, main_bus::Operation::Write(_))
            && matches!(
                before.phase,
                main_bus::Phase::S4 | main_bus::Phase::WaitHigh
            )
            && after.phase == main_bus::Phase::WaitLow
            && Some(next.waits) == pending.waits.checked_add(1)
        {
            self.full_wait = Some((pending, drive.half, drive.pass, view.half, view.pass));
        }
    }

    pub(super) fn finish(&mut self, view: BoardView) {
        let preview = self.preview.take().expect("resolved witness pass");
        assert_eq!((preview.0.half, preview.0.pass), (view.half, view.pass));
        self.sampled = Some(preview);
    }
}

impl<S: PhysicalCapture> Context<'_, S> {
    pub(crate) fn write_with_dma_witness(
        &mut self,
        cpu: &mut PrefetchCpu,
        write: DebugDataWrite,
    ) -> (CpuClocks, Witness) {
        assert!(self.fifo_witness.is_none());
        let begin = self.half_now().unwrap();
        self.fifo_witness = Some(Witness::default());
        let clocks = cpu.debug_data_write(self, write).unwrap();
        let witness = self.fifo_witness.take().unwrap();
        let receipt = witness.receipt.expect("actual MAIN completion");
        assert_eq!(receipt.end_half - begin, clocks.0 * 14);
        assert!(clocks.0 >= 4 + u64::from(receipt.waits));
        if let Some((half, _, source)) = witness.dma_pressure {
            assert!(half > begin && half <= receipt.end_half);
            assert!((0x300..0x380).contains(&source) && source & 1 == 0);
        }
        (clocks, witness)
    }

    pub(crate) fn write_with_fifo_witness(
        &mut self,
        cpu: &mut PrefetchCpu,
        write: DebugDataWrite,
    ) -> (CpuClocks, Witness) {
        assert!(self.fifo_witness.is_none());
        self.fifo_witness = Some(Witness::default());
        let clocks = cpu.debug_data_write(self, write).unwrap();
        let witness = self.fifo_witness.take().unwrap();
        let receipt = witness.receipt.expect("actual MAIN completion");
        assert_eq!(clocks.0, 4 + u64::from(receipt.waits));
        if let Some((pending, _, _, half, _)) = witness.full_wait {
            assert_eq!(pending.request, receipt.request);
            assert_eq!(pending.start_half, receipt.start_half);
            assert!(receipt.waits > pending.waits);
            assert!(half < receipt.sampled_ack_half);
        }
        (clocks, witness)
    }
}

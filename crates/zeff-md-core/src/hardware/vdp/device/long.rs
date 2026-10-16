use super::*;
use crate::hardware::vdp::VdpDmaSource;

impl Vdp {
    pub(super) fn preflight_dma_to(
        &self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
    ) -> Result<(), VdpScheduleError> {
        self.preflight_dma_from(at, region, acknowledge, None)
    }

    pub(super) fn preflight_dma_from(
        &self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<(), VdpScheduleError> {
        let mut state = self.state.clone();
        loop {
            // DMA data affects writes, not qualification; replay supplies the live bytes.
            let (next, _, _) =
                state.prepare_chunk_to_from(at, region, acknowledge, &self.memory, source)?;
            if next.now() == at {
                return Ok(());
            }
            debug_assert!(next.now() > state.now());
            state = next;
        }
    }

    fn replay_dma_from(
        &mut self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
        source: Option<VdpDmaSource<'_>>,
    ) -> Option<u8> {
        loop {
            let (state, writes, level) = self
                .state
                .prepare_chunk_to_from(at, region, acknowledge, &self.memory, source)
                .expect("validated DMA horizon");
            let complete = state.now() == at;
            self.publish(state, writes);
            if complete {
                return level;
            }
        }
    }

    pub(super) fn advance_dma_to(
        &mut self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
    ) -> Result<Option<u8>, VdpScheduleError> {
        self.advance_dma_from(at, region, acknowledge, None)
    }

    pub(super) fn advance_dma_from(
        &mut self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<Option<u8>, VdpScheduleError> {
        self.preflight_dma_from(at, region, acknowledge, source)?;
        Ok(self.replay_dma_from(at, region, acknowledge, source))
    }

    pub(super) fn advance_dma_with_line_from(
        &mut self,
        at: MasterTicks,
        capture_at: MasterTicks,
        acknowledge: bool,
        region: Region,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<Result<VdpScanline, VdpLineError>, VdpScheduleError> {
        self.preflight_dma_from(at, region, acknowledge, source)?;
        let captured = match self.preflight_dma_from(capture_at, region, false, source) {
            Ok(()) => {
                self.replay_dma_from(capture_at, region, acknowledge && capture_at == at, source);
                let result = self.prepare_line(region);
                if capture_at == at {
                    return Ok(result);
                }
                result
            }
            Err(error) => Err(error.into()),
        };
        self.replay_dma_from(at, region, acknowledge, source);
        Ok(captured)
    }

    #[inline(never)]
    pub(super) fn capture_dma_line_at(
        &mut self,
        at: MasterTicks,
        region: Region,
    ) -> Result<VdpScanline, VdpLineError> {
        self.preflight_dma_to(at, region, false)?;
        let checkpoint = self.checkpoint();
        self.replay_dma_from(at, region, false, None);
        let result = self.prepare_line(region);
        if result.is_err() {
            self.restore(checkpoint);
        }
        result
    }
}

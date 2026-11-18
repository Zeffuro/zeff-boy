use super::*;
use crate::hardware::vdp::owner::{BoardBoundary, BoardInput, BoardView, Effect, Storage};
#[derive(Clone)]
pub(super) struct Boundary<'a, P = (), H = ()> {
    pub physical: H,
    pub previous: P,
    pub board: Board,
    #[cfg(test)]
    pub fifo_witness: Option<fifo_witness::Witness>,
    #[cfg(test)]
    pub work_witness: Option<work_witness::Witness>,
    pub sound: SoundOwner,
    pub audio: &'a Audio,
    pub read: protocol::ReadContext<'a>,
}

impl<P: crate::hardware::vdp::owner::board::previous::TagSink, H: HalfCapture> BoardBoundary
    for Boundary<'_, P, H>
{
    #[cfg(test)]
    fn observe_dma_pressure(&mut self, half: u64, pass: u8, source: u32) {
        if let Some(witness) = &mut self.fifo_witness
            && (0x300..0x380).contains(&source)
            && source & 1 == 0
            && witness.dma_pressure.is_none()
        {
            witness.dma_pressure = Some((half, pass, source));
        }
    }

    const VDP_CAPTURE: bool = H::ENABLED;
    fn vdp(&mut self, event: crate::sound_trace::physical::vdp::Event) {
        self.physical.vdp(event);
    }
    #[cfg(test)]
    fn vdp_raw(&mut self, event: crate::sound_trace::physical::vdp::raw::Event) {
        self.physical.vdp_raw(event);
    }
    fn resolve(&mut self, view: BoardView) -> Result<BoardInput> {
        self.physical.at(crate::sound_trace::physical::Stamp {
            half: view.half,
            pass: view.pass,
        });
        if H::ENABLED {
            self.sound.advance_capturing(
                crate::hardware::vdp::owner::sound_cpu::AdvanceInput {
                    half: view.half,
                    pass: view.pass,
                    zclk: view.zclk,
                    response: self.board.sound_response(),
                    audio: self.audio,
                    irq: view.sound_interrupt,
                },
                &mut self.physical,
            )?;
        } else {
            self.sound.advance(
                view.half,
                view.pass,
                view.zclk,
                self.board.sound_response(),
                self.audio,
                view.sound_interrupt,
            )?;
        }
        self.board.qualify_companion(Companion {
            half: view.half,
            pass: view.pass,
            pins: self.sound.pins(),
            bank: self.sound.bank(),
        })?;
        if self
            .board
            .snapshot()
            .is_some_and(|snapshot| snapshot.pending.is_some())
            && let Some(binding) = self.board.protocol_request()
            && binding.request.operation == main_bus::Operation::Read
        {
            let (word, mask) = self.read.read(
                binding.request.access,
                binding.target,
                view.half / 2,
                &self.sound,
                self.audio,
            )?;
            self.board.qualify_protocol_drive(ProtocolDrive {
                binding,
                half: view.half,
                pass: view.pass,
                word,
                mask,
            })?;
        }
        #[cfg(test)]
        self.physical
            .vdp_raw(crate::sound_trace::physical::vdp::raw::Event::Companion(
                Companion {
                    half: view.half,
                    pass: view.pass,
                    pins: self.sound.pins(),
                    bank: self.sound.bank(),
                },
            ));
        #[cfg(test)]
        let before = self.board.snapshot();
        let input = self
            .board
            .resolve_inner_physical(view, &mut self.physical)?;
        #[cfg(test)]
        if let Some(witness) = &mut self.fifo_witness {
            witness.resolve(view, before, self.board.snapshot());
        }
        Ok(input)
    }

    fn finish_pass(&mut self, view: BoardView, storage: &impl Storage) -> Result<Vec<Effect>> {
        #[cfg(not(test))]
        let effects = self.board.finish_inner_physical(
            view,
            storage,
            &mut self.previous,
            &mut self.physical,
        )?;
        #[cfg(test)]
        let effects = if let Some(witness) = &mut self.work_witness {
            let mut pulse = None;
            let effects = self.board.finish_inner_with_work_witness(
                view,
                storage,
                &mut self.previous,
                &mut pulse,
                &mut self.physical,
            )?;
            if let Some(pulse) = pulse {
                witness.pulses.push(pulse);
            }
            effects
        } else {
            self.board.finish_inner_physical(
                view,
                storage,
                &mut self.previous,
                &mut self.physical,
            )?
        };
        #[cfg(test)]
        if let Some(witness) = &mut self.fifo_witness {
            witness.finish(view);
        }
        #[cfg(test)]
        if let Some(witness) = &mut self.work_witness
            && let Some(log) = &mut witness.companion_log
        {
            log.push(self.board.sound_companion().unwrap());
        }
        if let Some(receipt) = self.board.take_psg_write()? {
            self.sound.enqueue_psg(receipt)?;
            self.physical.psg(receipt);
        }
        Ok(effects)
    }
}

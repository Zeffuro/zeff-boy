use super::*;
use crate::sound_trace::SoundCapture;

impl SoundSchedule {
    pub(super) fn reserve_pending<C: SoundCapture>(
        &mut self,
        occupied: &[Reservation],
        capture: &mut C,
    ) -> Result<(), SoundFault> {
        let pending = self.pending.as_mut().unwrap();
        let checkpoint = if self.rmw_checkpoint.is_none()
            && (pending.bank_access.is_some() || pending.any_bank_access)
            && occupied.iter().any(|slot| {
                slot.requester == Requester::MAIN_RMW
                    && slot.ticks.start < pending.deadline
                    && pending.start < slot.ticks.end
            }) {
            Some(Box::new(pending.clone()))
        } else {
            None
        };
        pending.reserve_after(occupied)?;
        if let Some(checkpoint) = checkpoint {
            self.rmw_checkpoint = Some(checkpoint);
            capture.backup_pending();
        }
        Ok(())
    }

    pub(crate) fn finish_rmw<C: SoundCapture>(
        &mut self,
        reservation: &Reservation,
        now: u64,
        capture: &mut C,
    ) {
        let Some(mut checkpoint) = self.rmw_checkpoint.take() else {
            capture.finish_rmw(false);
            return;
        };
        if now >= reservation.ticks.end || self.fault.is_some() || self.pending.is_none() {
            capture.finish_rmw(false);
            return;
        }
        let completed = Reservation {
            requester: Requester::MAIN_RMW,
            ticks: reservation.ticks.start
                ..now.clamp(reservation.ticks.start, reservation.ticks.end),
        };
        match checkpoint.reserve_after(std::slice::from_ref(&completed)) {
            Ok(()) => {
                self.pending = Some(*checkpoint);
                capture.finish_rmw(true);
            }
            Err(fault) => {
                self.pending = None;
                capture.discard_pending();
                capture.finish_rmw(false);
                capture.fault();
                self.fault = Some(fault);
            }
        }
    }
}

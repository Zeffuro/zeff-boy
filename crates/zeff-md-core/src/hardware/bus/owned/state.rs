use super::*;
use crate::save_state::{read_section, write_section};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const TAG: &[u8; 8] = b"ZBUSOWN1";

impl Bus {
    pub(crate) fn input_state_tick(&self) -> Result<u64> {
        Ok(match self.vdp.owned_half() {
            Some(half) => main_bus::ceil_tick(half),
            None => self.now.get(),
        })
    }

    pub(in crate::hardware::bus) fn write_owned_state(
        &self,
        writer: &mut StateWriter,
    ) -> Result<()> {
        let Some(owner) = &self.owned else {
            return Ok(());
        };
        writer.write_bytes(TAG);
        write_section(writer, |writer| {
            writer.write_u64(owner.epoch_tick);
            writer.write_u64(owner.idle_clocks);
            writer.write_u64(owner.transfers);
            ensure!(owner.fault.is_none(), "owned bus has a runtime fault");
            writer.write_bool(owner.sound_event.is_some());
            if let Some(event) = owner.sound_event {
                event.write_state(writer);
            }
            writer.write_u8(owner.audio_pending.len() as u8);
            for event in &owner.audio_pending {
                event.write_state(writer);
            }
            write_section(writer, |writer| owner.board.write_state(writer))?;
            write_section(writer, |writer| owner.sound.write_state(writer))?;
            Ok(())
        })
    }

    pub(in crate::hardware::bus) fn read_owned_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<()> {
        if reader.is_exhausted() {
            ensure!(!self.vdp.is_owned(), "missing owned MAIN/SOUND state");
            self.owned = None;
            return Ok(());
        }
        validate_cartridge(&self.cartridge)?;
        let observation = self
            .vdp
            .owned_observation()?
            .ok_or_else(|| anyhow::anyhow!("owned bus without VDP owner"))?;
        let mut tag = [0; 8];
        reader.read_exact(&mut tag)?;
        ensure!(&tag == TAG, "unknown MAIN/SOUND owner extension");
        let mut value = OwnedState::software_origin();
        read_section(reader, |reader| {
            value.epoch_tick = reader.read_u64()?;
            value.idle_clocks = reader.read_u64()?;
            value.transfers = reader.read_u64()?;
            value.sound_event = if reader.read_bool()? {
                Some(SoundEvent::read_state(reader)?)
            } else {
                None
            };
            let count = reader.read_u8()?;
            ensure!(count <= 8, "owned audio event capacity");
            for _ in 0..count {
                value.audio_pending.push(AudioEffect::read_state(reader)?);
            }
            read_section(reader, |reader| {
                value.board = Board::read_state(reader, observation.half)?;
                Ok(())
            })?;
            read_section(reader, |reader| {
                value.sound = SoundOwner::read_state(reader)?;
                Ok(())
            })
        })?;
        ensure!(reader.is_exhausted(), "trailing MAIN/SOUND owner data");
        self.owned = Some(Box::new(value));
        Ok(())
    }

    pub(crate) fn validate_owned_state(&self) -> Result<()> {
        validate_cartridge(&self.cartridge)?;
        let owner = self
            .owned
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing bus owner"))?;
        let view = self
            .vdp
            .owned_observation()?
            .ok_or_else(|| anyhow::anyhow!("missing VDP owner"))?;
        ensure!(
            !self.clock_fault
                && !self.reset_output
                && self.rmw.is_none()
                && self.irq_level <= 7
                && self.ram.len() == WORK_RAM_SIZE,
            "invalid owned bus state"
        );
        ensure!(
            self.sound == SoundMemory::default(),
            "physical bus has a second SOUND owner"
        );
        self.input
            .validate_serial(main_bus::ceil_tick(view.half), self.region)?;
        self.vdp.validate_state(self.region)?;
        owner.board.validate_state()?;
        ensure!(
            owner.board.main_reset_released() && owner.board.protocol_request().is_none(),
            "owned MAIN has an unfinished transfer"
        );
        let main = owner
            .board
            .snapshot()
            .ok_or_else(|| anyhow::anyhow!("missing MAIN origin"))?;
        ensure!(
            main.phase == main_bus::Phase::Idle
                && main.pending.is_none()
                && main.completed.is_none(),
            "owned MAIN is not at CPU boundary"
        );
        let elapsed = self
            .cpu_clock
            .0
            .checked_add(owner.idle_clocks)
            .and_then(|clocks| clocks.checked_mul(14));
        ensure!(
            elapsed.and_then(|elapsed| main.origin_rise_half.checked_add(elapsed))
                == Some(view.half)
                && view.half == owner.board.half()
                && owner.epoch_tick == main_bus::ceil_tick(main.origin_rise_half),
            "owned CPU epoch differs"
        );
        ensure!(
            self.now.get() == view.half / 2
                && owner.board.observed_clocks() == view.board_clocks
                && owner.board.psg_level() == view.psg_write
                && owner.board.machine_boundary_ready(),
            "owned bus frontier differs"
        );
        owner.sound.validate()?;
        ensure!(
            owner.sound.frontier() == (view.half, 1, view.board_clocks[1]),
            "owned SOUND frontier differs"
        );
        let companion = owner
            .board
            .sound_companion()
            .ok_or_else(|| anyhow::anyhow!("missing SOUND companion"))?;
        ensure!(
            companion.half == view.half
                && companion.pass == 1
                && companion.pins == owner.sound.pins()
                && companion.bank == owner.sound.bank(),
            "owned SOUND companion differs"
        );
        ensure!(
            self.audio.state_now() == view.half / 2
                && !self.audio.batch_active()
                && self.audio.trace == Default::default()
                && self.audio.trace_origin.is_none(),
            "unowned audio frontier/trace"
        );
        ensure!(
            owner.audio_pending.len() <= 8
                && owner
                    .audio_pending
                    .iter()
                    .all(|event| view.half & 1 != 0 && event.tick() == view.half.div_ceil(2)),
            "stale owned audio effects"
        );
        if let Some(event) = owner.sound_event {
            event.validate()?;
            ensure!(
                event.source_half == view.half && event.tick == view.half.div_ceil(2),
                "stale MAIN SOUND event"
            );
        }
        ensure!(
            owner.fault.is_none() && owner.transfers <= self.cpu_clock.0 / 4,
            "invalid owned transfer/fault state"
        );
        self.video.validate_owned(view.raster_field, view.pixel)?;
        Ok(())
    }
}

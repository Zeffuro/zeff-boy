use super::*;
use crate::cartridge::SampledByte;
use crate::hardware::vdp::owner::domain::BoardBoundary;
use crate::hardware::vdp::owner::runtime::{OwnedObservation, PreparedHalf, Runtime};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

const OWNER_TAG: &[u8; 8] = b"ZVDPDOM1";
const MAX_OWNER_BYTES: usize = 128 * 1024;

impl Vdp {
    pub(crate) fn software_origin_owned(region: Region) -> Self {
        Self {
            state: State::bootstrap_mode5(),
            memory: VdpMemory::default(),
            interval_hint: None,
            dma_activity_epoch: 0,
            owned: Some(Box::new(Runtime::software_origin(region))),
        }
    }

    pub(crate) fn is_owned(&self) -> bool {
        self.owned.is_some()
    }

    pub(crate) fn owned_half(&self) -> Option<u64> {
        self.owned.as_ref().map(|runtime| runtime.half())
    }

    pub(crate) fn owned_observation(&self) -> Result<Option<OwnedObservation>> {
        self.owned
            .as_ref()
            .map(|runtime| runtime.observe())
            .transpose()
    }

    pub(crate) fn prepare_owned_half<'a, B: BoardBoundary, T: Into<SampledByte>>(
        &'a mut self,
        boundary: B,
        work: &'a mut [u8],
        main: impl Fn(u32) -> Result<T>,
    ) -> Result<PreparedHalf<'a, B>> {
        let runtime = self
            .owned
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("physical VDP profile is not selected"))?;
        runtime.prepare(
            boundary,
            &mut self.memory,
            work,
            |address| main(address).map(Into::into),
            &mut self.dma_activity_epoch,
        )
    }

    pub(super) fn require_legacy(&self) -> Result<(), VdpScheduleError> {
        if self.owned.is_some() {
            Err(
                crate::VdpInterruptError::Timing(crate::VdpTimingError::UnqualifiedDisplayHistory)
                    .into(),
            )
        } else {
            Ok(())
        }
    }

    pub(super) fn require_legacy_port(&self) -> Result<(), VdpPortError> {
        self.require_legacy().map_err(VdpPortError::Schedule)
    }

    pub(super) fn assert_legacy(&self) {
        assert!(
            self.owned.is_none(),
            "physical VDP requires its owned observation/transaction API"
        );
    }

    pub(super) fn validate_owned_state(&self, region: Region) -> Result<()> {
        let runtime = self.owned.as_ref().expect("physical VDP state");
        // The legacy prefix is inert and canonical on this profile.
        ensure!(
            self.state == State::bootstrap_mode5(),
            "physical VDP has a second legacy owner"
        );
        ensure!(
            self.interval_hint.is_none(),
            "physical VDP has a legacy interval"
        );
        ensure!(
            self.memory.known_cram().is_some(),
            "physical VDP has unknown CRAM"
        );
        runtime.validate_state(region)
    }

    pub(super) fn write_owned_state(&self, writer: &mut StateWriter) -> Result<()> {
        if let Some(runtime) = &self.owned {
            let mut encoded = StateWriter::new();
            runtime.write_state(&mut encoded)?;
            let encoded = encoded.into_bytes();
            ensure!(
                encoded.len() <= MAX_OWNER_BYTES,
                "physical VDP state exceeds size bound"
            );
            writer.write_bytes(OWNER_TAG);
            writer.write_u32(encoded.len() as u32);
            writer.write_bytes(&encoded);
        }
        Ok(())
    }

    pub(super) fn read_owned_state(
        reader: &mut StateReader<'_>,
        region: Region,
    ) -> Result<Option<Runtime>> {
        if reader.is_exhausted() {
            return Ok(None);
        }
        let mut tag = [0; 8];
        reader.read_exact(&mut tag)?;
        ensure!(&tag == OWNER_TAG, "unknown VDP state extension");
        let mut section = StateReader::new(reader.read_slice(MAX_OWNER_BYTES)?);
        let runtime = Runtime::read_state(&mut section, region)?;
        ensure!(
            section.is_exhausted() && reader.is_exhausted(),
            "trailing VDP owner state data"
        );
        Ok(Some(runtime))
    }
}

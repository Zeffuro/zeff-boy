use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl Pad {
    pub(in super::super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(match self.kind {
            ControllerKind::ThreeButton => 0,
            ControllerKind::SixButton => 1,
        });
        writer.write_u8(self.buttons);
        writer.write_u8(self.extra);
        writer.write_bool(self.compatibility);
        writer.write_u8(self.rises);
        writer.write_bool(self.last_edge.is_some());
        if let Some(edge) = self.last_edge {
            writer.write_u64(edge);
        }
    }

    pub(in super::super) fn read_state(reader: &mut StateReader<'_>, now: u64) -> Result<Self> {
        let kind = match reader.read_u8()? {
            0 => ControllerKind::ThreeButton,
            1 => ControllerKind::SixButton,
            _ => anyhow::bail!("invalid MD controller kind"),
        };
        let candidate = Self {
            kind,
            buttons: reader.read_u8()?,
            extra: reader.read_u8()?,
            compatibility: reader.read_bool()?,
            rises: reader.read_u8()?,
            last_edge: if reader.read_bool()? {
                Some(reader.read_u64()?)
            } else {
                None
            },
        };
        ensure!(
            candidate.extra & !PAD_EXTRA_MASK == 0,
            "invalid MD controller extra buttons"
        );
        ensure!(
            candidate.rises <= PAD_SIX_MAX_RISES,
            "invalid MD controller handshake"
        );
        ensure!(
            candidate.last_edge.is_none_or(|edge| edge <= now),
            "future MD controller edge"
        );
        ensure!(
            candidate.rises == 0 || candidate.last_edge.is_some(),
            "missing MD controller edge"
        );
        ensure!(
            kind == ControllerKind::SixButton
                || (!candidate.compatibility
                    && candidate.rises == 0
                    && candidate.last_edge.is_none()),
            "invalid three-button controller handshake"
        );
        ensure!(
            !candidate.compatibility || (candidate.rises == 0 && candidate.last_edge.is_none()),
            "invalid MD compatibility handshake"
        );
        Ok(candidate)
    }
}

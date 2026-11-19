use super::super::model::Phase;
use super::{FunctionalClock, FunctionalProfile, InstructionModel};
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl InstructionModel {
    pub(in super::super) fn write_functional_clock(
        &self,
        w: &mut StateWriter,
    ) -> Result<(), super::Error> {
        let Some(clock) = &self.clock else {
            return Ok(());
        };
        if clock.hold {
            return Err(super::Error::Busy);
        }
        w.write_bytes(b"ZSVPFNC1");
        w.write_u64(clock.profile.identity);
        w.write_u32(clock.profile.numerator);
        w.write_u32(clock.profile.denominator);
        for cost in clock.profile.costs {
            w.write_u16(cost);
        }
        w.write_u64(clock.origin);
        w.write_u64(clock.now);
        w.write_u32(clock.fraction);
        w.write_u16(clock.remaining);
        w.write_u64(clock.grants);
        Ok(())
    }

    pub(in super::super) fn read_functional_clock(
        &self,
        r: &mut StateReader<'_>,
    ) -> anyhow::Result<Option<FunctionalClock>> {
        let Some(expected) = &self.clock else {
            return Ok(None);
        };
        let mut tag = [0; 8];
        r.read_exact(&mut tag)?;
        anyhow::ensure!(&tag == b"ZSVPFNC1", "missing functional clock owner");
        let identity = r.read_u64()?;
        let numerator = r.read_u32()?;
        let denominator = r.read_u32()?;
        let mut costs = [0; 6];
        for cost in &mut costs {
            *cost = r.read_u16()?;
        }
        let profile = FunctionalProfile::new(identity, numerator, denominator, costs)?;
        anyhow::ensure!(
            profile == expected.profile,
            "functional clock profile differs"
        );
        Ok(Some(FunctionalClock {
            profile,
            origin: r.read_u64()?,
            now: r.read_u64()?,
            fraction: r.read_u32()?,
            remaining: r.read_u16()?,
            grants: r.read_u64()?,
            hold: false,
        }))
    }

    pub(in super::super) fn validate_functional_clock(&self) -> anyhow::Result<()> {
        let Some(clock) = &self.clock else {
            return Ok(());
        };
        anyhow::ensure!(
            self.services.is_some() && !clock.hold,
            "functional service owner"
        );
        anyhow::ensure!(
            !matches!(self.phase, Phase::Commit { .. }),
            "unsettled functional commit"
        );
        anyhow::ensure!(clock.origin <= clock.now, "functional clock origin");
        let elapsed = u128::from(clock.now - clock.origin) * u128::from(clock.profile.numerator);
        anyhow::ensure!(
            u128::from(clock.fraction) == elapsed % u128::from(clock.profile.denominator),
            "functional clock phase"
        );
        anyhow::ensure!(
            u128::from(clock.grants) <= elapsed / u128::from(clock.profile.denominator),
            "functional grant count"
        );
        let minimum = if matches!(self.phase, Phase::Reset) {
            0
        } else {
            self.instructions
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("functional instruction count"))?
        };
        anyhow::ensure!(
            clock.grants >= minimum && (1..=self.functional_cost()?).contains(&clock.remaining),
            "functional pending cost/count"
        );
        if matches!(self.phase, Phase::Reset) {
            anyhow::ensure!(clock.grants == 0, "functional reset grants");
        }
        Ok(())
    }
}

use super::Emulator;
use zeff_emu_common::address::Address;
use zeff_emu_common::audio_trace::AudioTraceInvalidation;
use zeff_emu_common::cheats::{
    CheatApplyStats, CheatByteTarget, CheatPatch, CheatType, apply_wide_ram_cheats,
};

mod formats;
mod rom;
pub use formats::{decode_game_genie, parse_cheats};

fn canonical_address(patch: &CheatPatch) -> bool {
    matches!(
        patch,
        CheatPatch::WideRamWrite { address, .. }
            | CheatPatch::WideRamWriteIfEquals { address, .. }
            if (0xff0000..=0xffffff).contains(address)
    )
}

struct WorkRam<'a>(&'a mut [u8]);

impl CheatByteTarget<Address> for WorkRam<'_> {
    fn cheat_peek8(&self, address: Address) -> u8 {
        self.0[address as usize & 0xffff]
    }

    fn cheat_write8(&mut self, address: Address, value: u8) {
        self.0[address as usize & 0xffff] = value;
    }
}

impl Emulator {
    pub fn apply_ram_cheats(&mut self, patches: &[CheatPatch]) -> CheatApplyStats {
        let mut stats = CheatApplyStats::default();
        let mut target = WorkRam(&mut self.bus.ram);
        for patch in patches {
            if !canonical_address(patch) {
                stats.ignored += 1;
                continue;
            }
            let applied = apply_wide_ram_cheats(&mut target, std::slice::from_ref(patch));
            stats.applied += applied.applied;
            stats.skipped_compare += applied.skipped_compare;
        }
        if stats.applied != 0 {
            self.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
        }
        stats
    }
}

pub fn parse_raw_cheats(input: &str) -> Option<Vec<CheatPatch>> {
    let (patches, format) = parse_cheats(input)?;
    (format == CheatType::Raw && patches.iter().all(canonical_address)).then_some(patches)
}

#[cfg(test)]
mod rom_tests;
#[cfg(test)]
mod tests;

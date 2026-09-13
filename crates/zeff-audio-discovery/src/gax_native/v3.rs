use super::{Budget, GaxNativeSong, RomSpan, ScanStop, startup};

pub(super) mod driver;
mod profiles;
mod songs;
pub(crate) use profiles::is_init;

#[cfg(test)]
const MAX_RETAINED_BYTES: usize = super::MAX_RETAINED_BYTES;

pub(super) fn scan(
    bytes: &[u8],
    output: &mut Vec<GaxNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
    retained_limit: usize,
) -> Result<(), ScanStop> {
    let (images, profiles) = profiles::recognize(bytes, budget)?;
    if profiles.is_empty() {
        return Ok(());
    }
    let mut retained = super::retained_bytes(output);
    if retained > retained_limit {
        return Err(ScanStop::InventoryLimit);
    }
    for at in (0..bytes.len().saturating_sub(199)).step_by(4) {
        if at % 32 == 0 {
            budget.charge()?;
        }
        let image = startup::image_start(&images, at);
        let mut matches = profiles.iter().filter(|profile| profile.image == image);
        let Some(profile) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let Some(mut song) = songs::read(bytes, at, budget)? else {
            continue;
        };
        if output.len() >= max_candidates || output.len() > u16::MAX as usize {
            return Err(ScanStop::CandidateLimit);
        }
        song.spans.extend(profile.spans.iter().copied());
        super::merge_spans(&mut song.spans);
        let song = GaxNativeSong {
            header: RomSpan::new(at, 200),
            index: output.len() as u16,
            title: song.title,
            channels: song.channels,
            native: profile.native.clone(),
            mapped_spans: song.spans,
            warnings: Vec::new(),
        };
        super::push_song(output, song, &mut retained, retained_limit)?;
    }
    Ok(())
}

pub(super) fn build(bytes: &[u8], song: &GaxNativeSong) -> anyhow::Result<Vec<u8>> {
    driver::build(bytes, song)
}

pub(crate) struct VersionBinding {
    images: Vec<usize>,
    profiles: Vec<profiles::Profile>,
}

impl VersionBinding {
    pub fn recognize(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Self, ScanStop> {
        let (images, profiles) = profiles::recognize(bytes, budget)?;
        Ok(Self { images, profiles })
    }
    pub fn at(&self, offset: usize) -> Option<(&str, RomSpan)> {
        let image = startup::image_start(&self.images, offset);
        let mut matches = self
            .profiles
            .iter()
            .filter(|profile| profile.image == image);
        let profile = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some((&profile.native.version, profile.version_span))
    }
}

#[cfg(test)]
mod tests;

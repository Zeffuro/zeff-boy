use serde::Serialize;

use super::{Budget, Confidence, RomSpan, ScanStop, SongCandidate, ToneInventory, Warning, word};

const fn signature<const N: usize>(hex: &str) -> [u8; N] {
    match const_hex::const_decode_to_array(hex.as_bytes()) {
        Ok(bytes) => bytes,
        Err(_) => panic!("invalid Camelot opcode signature"),
    }
}

const SWITCH: &[u8] = &signature::<16>("0220b0e1400080030100c4051c4084e2");
const PULSE: &[u8] = &signature::<40>(
    "0260d3e5062c82e00460d3e5066c92e00660e04126a4a0e10310d3e50100d3e5000ca0e19a0126e0",
);
const SAW: &[u8] = &signature::<136>(
    "01c05ce22000001a036ca0e3abb0a0e1ffbccbe370c0a0e3034495e8847197e0279c6ce08760a0e1a69d49e0c22099e09b022010847197e0279c6ce08760a0e1a69d49e0c22099e09b122110847197e0279c6ce08760a0e1a69d49e0c22099e09ba22a10847197e0279c6ce08760a0e1a69d49e0c22099e09be22e100344a5e8048058e2e3ffffca",
);
const TRIANGLE: &[u8] = &signature::<88>(
    "8060a0e306cda0e3034495e8847197e0c79b6650a79b4c409b0920e0847197e0c79b6650a79b4c409b1921e0847197e0c79b6650a79b4c409ba92ae0847197e0c79b6650a79b4c409be92ee00344a5e8048058e2ebffffca",
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SynthRecipe {
    pub profile: &'static str,
    pub engine_evidence: [RomSpan; 4],
    pub header: RomSpan,
    pub parameters: RomSpan,
    pub frequency: u32,
    pub kind: SynthKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SynthKind {
    Pulse {
        base_duty: u8,
        lfo_step: u8,
        modulation: u8,
        lfo_offset: u8,
    },
    PseudoSaw,
    Triangle,
}

pub(crate) fn enrich(
    bytes: &[u8],
    songs: &mut [SongCandidate],
    budget: &mut Budget<'_>,
) -> Result<(), ScanStop> {
    if !songs.iter().any(|song| {
        song.warnings
            .iter()
            .any(|warning| matches!(warning, Warning::EmptySample { .. }))
    }) {
        return Ok(());
    }
    let Some(engine) = recognize(bytes, budget)? else {
        return Ok(());
    };
    for song in songs {
        let mut resolved = std::collections::BTreeSet::new();
        for instrument in &mut song.instruments {
            budget.charge()?;
            let voice = instrument.voice;
            if let Some(header) = attach(bytes, &mut instrument.tone, engine) {
                resolved.insert((voice, header));
            }
            for region in &mut instrument.regions {
                budget.charge()?;
                if let Some(tone) = &mut region.tone
                    && let Some(header) = attach(bytes, tone, engine)
                {
                    resolved.insert((voice, header));
                    if matches!(region.warning, Some(Warning::EmptySample { .. })) {
                        region.warning = None;
                    }
                }
            }
        }
        song.warnings.retain(|warning| !matches!(warning, Warning::EmptySample { voice, offset } if resolved.contains(&(*voice, *offset))));
        song.evidence.validated_instruments = song
            .instruments
            .iter()
            .filter(|instrument| {
                !song.warnings.iter().any(|warning| match warning {
                    Warning::UnsupportedInstrument { voice, .. }
                    | Warning::InvalidInstrument { voice }
                    | Warning::EmptySample { voice, .. }
                    | Warning::UnresolvedInstrumentKeys { voice }
                    | Warning::InvalidInstrumentRegion { voice, .. }
                    | Warning::UnsupportedInstrumentRegion { voice, .. } => {
                        *voice == instrument.voice
                    }
                    _ => false,
                })
            })
            .count() as u16;
        if song.warnings.is_empty() && song.evidence.validated_instruments > 0 {
            song.confidence = Confidence::Structural;
        }
    }
    Ok(())
}

fn recognize(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<[RomSpan; 4]>, ScanStop> {
    let mut found = None;
    for start in (0..bytes.len().saturating_sub(SWITCH.len() - 1)).step_by(4) {
        budget.charge()?;
        if bytes.get(start..start + SWITCH.len()) != Some(SWITCH) {
            continue;
        }
        let mut spans = [RomSpan::new(start, SWITCH.len()); 4];
        let mut cursor = start + SWITCH.len();
        for (index, signature) in [PULSE, SAW, TRIANGLE].into_iter().enumerate() {
            let end = bytes.len().min(start + 4096);
            let mut matched = None;
            for offset in (cursor..end.saturating_sub(signature.len() - 1)).step_by(4) {
                budget.charge()?;
                if bytes.get(offset..offset + signature.len()) == Some(signature) {
                    matched = Some(offset);
                    break;
                }
            }
            let Some(offset) = matched else {
                break;
            };
            spans[index + 1] = RomSpan::new(offset, signature.len());
            cursor = offset + signature.len();
        }
        if spans[3].byte_len != TRIANGLE.len() as u32 {
            continue;
        }
        if found.is_some() {
            return Ok(None);
        }
        found = Some(spans);
    }
    Ok(found)
}

fn attach(bytes: &[u8], tone: &mut ToneInventory, engine: [RomSpan; 4]) -> Option<u32> {
    if tone.kind != 0 || tone.sample.is_some() {
        return None;
    }
    let header = tone.sample_header?;
    let offset = header.effective_offset as usize;
    if word(bytes, offset)? != 0x4000_0000
        || word(bytes, offset + 8)? != 0
        || word(bytes, offset + 12)? != 0
    {
        return None;
    }
    let frequency = word(bytes, offset + 4)?;
    if frequency == 0 || bytes.get(offset + 16)? != &0x80 {
        return None;
    }
    let (kind, length) = match bytes.get(offset + 17)? {
        0 => {
            let values = bytes.get(offset + 18..offset + 22)?;
            (
                SynthKind::Pulse {
                    base_duty: values[0],
                    lfo_step: values[1],
                    modulation: values[2],
                    lfo_offset: values[3],
                },
                6,
            )
        }
        1 => (SynthKind::PseudoSaw, 2),
        2 => (SynthKind::Triangle, 2),
        _ => return None,
    };
    tone.synthesis = Some(SynthRecipe {
        profile: "camelot-mp2k-synth/1",
        engine_evidence: engine,
        header,
        parameters: RomSpan::new(offset + 16, length),
        frequency,
        kind,
    });
    Some(header.effective_offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::camelot::synth_fixture;
    use std::sync::atomic::AtomicBool;

    fn report(bytes: &[u8]) -> crate::ScanReport {
        crate::scan(
            zeff_emu_common::system::System::Gba,
            bytes,
            Default::default(),
            &AtomicBool::new(false),
        )
    }

    #[test]
    fn engine_fragments_and_bounded_recipes_are_all_required() {
        for kind in 0..=2 {
            let bytes = synth_fixture(kind);
            let scan = report(&bytes);
            let recipe = scan.candidates[0].instruments[0]
                .synthesis
                .as_ref()
                .unwrap();
            assert_eq!(recipe.parameters.byte_len, if kind == 0 { 6 } else { 2 });
            assert_eq!(scan.candidates[0].confidence, Confidence::Structural);
            assert!(scan.candidates[0].instruments[0].sample.is_none());
            for at in [0x1800, 0x1900, 0x1A00, 0x1B00, 0x310] {
                let mut bad = bytes.clone();
                bad[at] ^= 1;
                assert!(
                    report(&bad).candidates[0].instruments[0]
                        .synthesis
                        .is_none()
                );
            }
        }
        assert!(
            report(&synth_fixture(3)).candidates[0].instruments[0]
                .synthesis
                .is_none()
        );
        let bytes = synth_fixture(0);
        let mut budget = Budget {
            remaining: 0,
            cancel: &AtomicBool::new(false),
        };
        assert_eq!(recognize(&bytes, &mut budget), Err(ScanStop::WorkLimit));
    }

    #[test]
    fn recipe_bounds_exclude_padding_and_duplicate_engines_are_ambiguous() {
        for kind in 0..=2 {
            let bytes = synth_fixture(kind);
            let scan = report(&bytes);
            let mut tone = scan.candidates[0].instruments[0].tone.clone();
            let engine = tone.synthesis.take().unwrap().engine_evidence;
            let end = if kind == 0 { 0x316 } else { 0x312 };
            assert!(attach(&bytes[..end - 1], &mut tone, engine).is_none());
            assert_eq!(attach(&bytes[..end], &mut tone, engine), Some(0x300));
            assert_eq!(
                tone.synthesis.unwrap().parameters.byte_len as usize,
                end - 0x310
            );
        }
        let mut bytes = synth_fixture(0);
        bytes.resize(0x5000, 0xFF);
        let duplicate = bytes[0x1800..0x1C00].to_vec();
        bytes[0x3800..0x3C00].copy_from_slice(&duplicate);
        assert!(
            report(&bytes).candidates[0].instruments[0]
                .synthesis
                .is_none()
        );
    }
}

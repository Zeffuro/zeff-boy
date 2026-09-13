pub(crate) use zeff_audio_discovery::test_support::*;

pub(crate) fn rom_span(offset: usize, len: usize) -> super::RomSpan {
    assert!(offset <= super::MAX_ROM_BYTES && len <= super::MAX_ROM_BYTES - offset);
    super::RomSpan {
        effective_offset: offset as u32,
        byte_len: len as u32,
        canonical_cpu_address: 0x0800_0000 + offset as u32,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn cdda_fixture() -> anyhow::Result<(
    std::sync::Arc<super::media::ScanInput>,
    super::media::ScanManifest,
    Vec<i16>,
)> {
    use super::cdda::{CdAudioInput, CdAudioProvenance};
    use std::sync::{Arc, atomic::AtomicBool};
    use zeff_pce_core::hardware::{CdDisc, CdTrack, CdTrackMode};
    let pcm = (0..588 * 12)
        .flat_map(|frame| {
            let value = ((frame * 43) % 30_000) as i16;
            [value, -value]
        })
        .collect::<Vec<_>>();
    let mut stored = vec![0xBB; 2352];
    stored.extend(pcm.iter().flat_map(|sample| sample.to_le_bytes()));
    let disc = Arc::new(CdDisc::new(vec![
        CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048])?,
        CdTrack::from_stored_data(2, 0, Some(1), 2, CdTrackMode::Audio, stored)?,
    ])?);
    let hash = const_hex::encode(disc.content_hash());
    let cdda = Arc::new(CdAudioInput::new(
        Arc::clone(&disc),
        hash.clone(),
        disc.payload_len(),
        hash.clone(),
        CdAudioProvenance {
            source_kind: "synthetic",
            source_media_sha256: hash,
            source_media_len: disc.payload_len().unwrap(),
            selected_member_path_sha256: None,
            transforms_applied: false,
        },
    )?);
    let input = Arc::new(super::media::ScanInput {
        cdda: Some(cdda),
        system: Some(zeff_emu_common::system::System::Pce),
        standalone_audio: None,
        bytes: Vec::new().into(),
        provenance: None,
        analysis_profile: "synthetic-cd-audio",
        display_name: Some("Synthetic CD".into()),
    });
    let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
    Ok((input, manifest, pcm))
}

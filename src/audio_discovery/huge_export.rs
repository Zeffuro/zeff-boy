use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::{Result, ensure};
use serde_json::json;
use zeff_audio_discovery::{ScanLimits, huge::discovery};

use super::audio_file::{self, AudioData};

const MAX_SONGS: usize = 8;
const MAX_TOTAL_FRAMES: u32 = 4096;
const SAMPLE_RATE: u32 = 48_000;

pub(crate) fn write_new(
    path: &Path,
    source: &[u8],
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<usize> {
    ensure!(!cancel.load(Ordering::Relaxed), "hUGE export cancelled");
    ensure!(
        !path.exists(),
        "hUGE output already exists: {}",
        path.display()
    );
    let discovery = discovery::discover(
        source,
        ScanLimits {
            max_candidates: 64,
            ..Default::default()
        },
        cancel,
    )
    .map_err(|stop| anyhow::anyhow!("hUGE discovery stopped: {stop:?}"))?;
    let mut descriptors = BTreeMap::new();
    for bound in &discovery.bound {
        descriptors
            .entry(u16::try_from(bound.song.descriptor.offset)?)
            .or_insert(bound.song.loop_ticks);
    }
    ensure!(
        !descriptors.is_empty(),
        "hUGE discovery found no strict bound selections"
    );
    ensure!(
        descriptors.len() <= MAX_SONGS,
        "hUGE discovery found more than {MAX_SONGS} selections"
    );

    let mut total_frames = 0u32;
    let mut selected = Vec::with_capacity(descriptors.len());
    let selection_count = descriptors.len();
    for (descriptor, loop_ticks) in descriptors {
        let frames = super::huge_validation::required_frames(loop_ticks)?;
        total_frames = total_frames
            .checked_add(frames)
            .ok_or_else(|| anyhow::anyhow!("hUGE validation frame budget overflow"))?;
        ensure!(
            total_frames <= MAX_TOTAL_FRAMES,
            "hUGE selections exceed the {MAX_TOTAL_FRAMES}-frame validation budget"
        );
        selected.push((descriptor, frames));
    }

    let mut outputs = Vec::with_capacity(selected.len());
    for (index, (descriptor, frames)) in selected.into_iter().enumerate() {
        ensure!(!cancel.load(Ordering::Relaxed), "hUGE export cancelled");
        let validated = super::huge_validation::validate(source, descriptor, frames, cancel)?;
        ensure!(
            validated.pcm.len().is_multiple_of(2) && !validated.pcm.is_empty(),
            "hUGE validation returned invalid stereo PCM"
        );
        let proof = serde_json::to_vec_pretty(&validated.report)?;
        let pcm = validated
            .pcm
            .iter()
            .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
            .collect::<Vec<_>>();
        let wav = audio_file::encode(
            super::formats::AudioFormat::Wav,
            AudioData {
                pcm: &pcm,
                channels: 2,
                sample_rate: SAMPLE_RATE,
                loop_range: None,
                pitch: None,
            },
            &proof,
            cancel,
        )?;
        outputs.push((descriptor, frames, proof, wav, validated.isolated));
        progress.store(
            ((index + 1) * 80 / selection_count)
                .try_into()
                .unwrap_or(80),
            Ordering::Relaxed,
        );
    }

    let mut bundle = super::bundle::Bundle::new();
    let mut songs = Vec::with_capacity(outputs.len());
    for (descriptor, frames, proof, wav, isolated) in outputs {
        let prefix = format!("song-{descriptor:04x}");
        let wav_path = format!("{prefix}/music.wav");
        let rom_path = format!("{prefix}/isolated.gb");
        let proof_path = format!("{prefix}/proof.json");
        bundle.add(&wav_path, &wav)?;
        bundle.add(&rom_path, &isolated)?;
        bundle.add(&proof_path, &proof)?;
        songs.push(json!({
            "descriptor": descriptor,
            "validation_frames": frames,
            "music_wav": {"path": wav_path, "sha256": zeff_firmware::sha256_hex(&wav)},
            "isolated_rom": {"path": rom_path, "sha256": zeff_firmware::sha256_hex(&isolated)},
            "proof": {"path": proof_path, "sha256": zeff_firmware::sha256_hex(&proof)},
        }));
    }
    let manifest = json!({
        "schema": "zeff-huge-isolated-export/1",
        "source": {"byte_len": source.len(), "sha256": zeff_firmware::sha256_hex(source)},
        "discovery": discovery,
        "runtime_budget": {
            "max_candidates": 64,
            "max_selections": MAX_SONGS,
            "max_total_validation_frames": MAX_TOTAL_FRAMES,
            "sample_rate": SAMPLE_RATE,
        },
        "songs": songs,
        "limitations": [
            "Each archived selection passed the bounded original-versus-isolated execution and control-state proof recorded in its proof file.",
            "This export is an experimental isolated DMG playback artifact. It does not add a catalog selection, preview, GBS container, general native export, complete soundtrack claim, or hardware-equivalence claim.",
            "Validation is limited to the selected strict hUGEDriver revision, mapping, bootstrap, supported song structure, and bounded recurrence interval."
        ],
    });
    bundle.add("manifest.json", &serde_json::to_vec_pretty(&manifest)?)?;
    super::assets::publish_bytes(path, &bundle.finish()?, cancel, progress)?;
    Ok(manifest["songs"].as_array().map_or(0, Vec::len))
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn entry(zip: &mut zip::ZipArchive<std::fs::File>, path: &str) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        zip.by_name(path)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn wav_pcm(wav: &[u8]) -> Result<&[u8]> {
        ensure!(
            wav.starts_with(b"RIFF") && wav.get(8..12).is_some_and(|tag| tag == b"WAVE"),
            "invalid WAV header"
        );
        let mut offset = 12;
        while let Some(header) = wav.get(offset..offset + 8) {
            let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let data = wav
                .get(offset + 8..offset + 8 + length)
                .ok_or_else(|| anyhow::anyhow!("truncated WAV chunk"))?;
            if &header[..4] == b"data" {
                return Ok(data);
            }
            offset = offset + 8 + length + length % 2;
        }
        anyhow::bail!("WAV has no data chunk")
    }

    #[test]
    fn publishes_only_the_validated_isolated_selection() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let source = zeff_audio_discovery::huge::fixture::rom();
        let output = directory.path().join("huge.zip");
        assert_eq!(
            write_new(
                &output,
                &source,
                &AtomicBool::new(false),
                &AtomicU32::new(0),
            )?,
            1
        );

        let mut zip = zip::ZipArchive::new(std::fs::File::open(output)?)?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&entry(&mut zip, "manifest.json")?)?;
        assert_eq!(
            manifest["source"]["sha256"],
            zeff_firmware::sha256_hex(&source)
        );
        assert_eq!(
            manifest["discovery"]["bound"]
                .as_array()
                .map(|items| items.len()),
            Some(1)
        );
        let song = &manifest["songs"][0];
        assert_eq!(song["descriptor"], 0x200);
        assert_eq!(song["validation_frames"], 641);
        for field in ["music_wav", "isolated_rom", "proof"] {
            let bytes = entry(&mut zip, song[field]["path"].as_str().unwrap())?;
            assert_eq!(song[field]["sha256"], zeff_firmware::sha256_hex(&bytes));
        }
        let wav = entry(&mut zip, song["music_wav"]["path"].as_str().unwrap())?;
        assert!(wav.starts_with(b"RIFF") && wav.get(8..12).is_some_and(|tag| tag == b"WAVE"));
        let proof: serde_json::Value =
            serde_json::from_slice(&entry(&mut zip, song["proof"]["path"].as_str().unwrap())?)?;
        assert_eq!(proof["passed"], true);
        assert_eq!(proof["original"]["update_count"], 641);
        assert_eq!(proof["original"]["update_period_cycles"], 70_224);
        assert_eq!(
            proof["original"]["pcm_s16_sha256"],
            zeff_firmware::sha256_hex(wav_pcm(&wav)?)
        );
        Ok(())
    }

    #[test]
    fn rejects_cancelled_or_changed_bootstrap_without_publishing() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let source = zeff_audio_discovery::huge::fixture::rom();
        let existing = directory.path().join("existing.zip");
        std::fs::write(&existing, b"keep")?;
        assert!(
            write_new(
                &existing,
                &source,
                &AtomicBool::new(true),
                &AtomicU32::new(0),
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&existing)?, b"keep");
        assert!(
            write_new(
                &existing,
                &source,
                &AtomicBool::new(false),
                &AtomicU32::new(0)
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&existing)?, b"keep");

        let missing = directory.path().join("no-driver.zip");
        assert!(
            write_new(
                &missing,
                &[0; 0x8000],
                &AtomicBool::new(false),
                &AtomicU32::new(0)
            )
            .is_err()
        );
        assert!(!missing.exists());

        let mut changed = source;
        changed[0x150] = 0;
        let output = directory.path().join("changed.zip");
        assert!(
            write_new(
                &output,
                &changed,
                &AtomicBool::new(false),
                &AtomicU32::new(0),
            )
            .is_err()
        );
        assert!(!output.exists());
        Ok(())
    }
}

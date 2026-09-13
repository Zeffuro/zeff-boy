use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::{Value, json};

use super::audio_file::{self, AudioData};
use super::bank::InstrumentBank;
use super::bundle::Bundle;
use super::formats::AudioFormat;

pub(super) fn encode(
    bank: &InstrumentBank,
    mut metadata: Value,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    super::bank::validate(bank)?;
    let mut bundle = Bundle::new();
    let mut mappings = Vec::new();
    for preset in bank.presets.iter().filter(|preset| preset.bank != 128) {
        ensure!(!cancel.load(Ordering::Relaxed), "export cancelled");
        let path = format!(
            "instruments/bank-{:03}-program-{:03}.sfz",
            preset.bank, preset.program
        );
        let mut text = "// zeff-boy approximate instrument projection\n// See ../manifest.json for source identity and limitations.\n".to_owned();
        for region in &preset.zones {
            let sample = &bank.samples[region.sample_index];
            let correction = sample.pitch_correction() + region.fine_tune;
            let transpose = region.coarse_tune + correction.div_euclid(100);
            let tune = correction.rem_euclid(100);
            let sustain_db = if region.sustain_level > 0.0 {
                (-20.0 * region.sustain_level.log10()).clamp(0.0, 100.0)
            } else {
                100.0
            };
            let release = region.release_seconds * (100.0 - sustain_db) / 100.0;
            ensure!(
                (-127..=127).contains(&transpose),
                "SFZ transpose is out of range"
            );
            writeln!(
                text,
                "<region> sample=../samples/sample-{:04}.wav lokey={} hikey={} pitch_keycenter={} pitch_keytrack={} transpose={} tune={} pan={:.6} ampeg_attack={:.9} ampeg_decay={:.9} ampeg_sustain={:.6} ampeg_release={:.9}",
                region.sample_index,
                region.key_start,
                region.key_end,
                region.root_key,
                region.scale_tuning,
                transpose,
                tune,
                f64::from(region.pan) / 5.0,
                region.attack_seconds,
                region.decay_seconds,
                region.sustain_level * 100.0,
                release
            )?;
            if let Some((start, end)) = sample.loop_range {
                writeln!(
                    text,
                    "loop_mode=loop_continuous loop_start={start} loop_end={}",
                    end - 1
                )?;
            } else {
                writeln!(text, "loop_mode=no_loop")?;
            }
        }
        bundle.add(&path, text.as_bytes())?;
        mappings.push(
            json!({"path":path,"bank":preset.bank,"program":preset.program,"name":preset.name}),
        );
    }
    for (index, sample) in bank.samples.iter().enumerate() {
        ensure!(!cancel.load(Ordering::Relaxed), "export cancelled");
        let bytes = audio_file::encode(
            AudioFormat::Wav,
            AudioData {
                pcm: &sample.pcm,
                channels: 1,
                sample_rate: sample.integer_rate(),
                loop_range: sample.loop_range,
                pitch: Some((sample.original_pitch, sample.pitch_correction())),
            },
            b"{}",
            cancel,
        )?;
        bundle.add(&format!("samples/sample-{index:04}.wav"), &bytes)?;
    }
    metadata["kind"] = json!("sfz_instrument_projection");
    metadata["programs"] = json!(mappings);
    metadata["sfz_usage"] = json!(
        "Load the SFZ matching each MIDI program into the receiving sampler track. SFZ defines individual instruments; it is not a MIDI bank container. Percussion aliases use the same program files."
    );
    metadata["sfz_samples"] = json!(bank.samples.iter().enumerate().map(|(index, sample)| json!({"path":format!("samples/sample-{index:04}.wav"),"frames":sample.pcm.len(),"exact_sample_rate":sample.sample_rate,"root_key":sample.original_pitch,"loop_range":sample.loop_range})).collect::<Vec<_>>());
    bundle.add("manifest.json", &serde_json::to_vec_pretty(&metadata)?)?;
    bundle.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_discovery::bank::{Preset, Region, Sample};
    use std::io::{Cursor, Read};

    #[test]
    fn sfz_pack_shares_original_samples_and_maps_region_pitch_envelope_and_loops() -> Result<()> {
        let pcm = vec![-32768, 0, 16384, 32512];
        let region = Region {
            key_start: 40,
            key_end: 50,
            sample_index: 0,
            root_key: 48,
            scale_tuning: 0,
            coarse_tune: -2,
            fine_tune: -17,
            pan: -250,
            attack_seconds: 0.01,
            decay_seconds: 0.2,
            sustain_level: 0.1,
            release_seconds: 1.0,
        };
        let bank = InstrumentBank {
            name: "Test".into(),
            comment: "{}".into(),
            samples: vec![Sample {
                name: "Source".into(),
                pcm: pcm.clone(),
                sample_rate: 8000.0,
                original_pitch: 60,
                loop_range: Some((1, 4)),
            }],
            presets: vec![
                Preset {
                    name: "Voice".into(),
                    bank: 0,
                    program: 7,
                    zones: vec![region.clone()],
                },
                Preset {
                    name: "Voice drums".into(),
                    bank: 128,
                    program: 7,
                    zones: vec![region],
                },
            ],
        };
        let data = encode(
            &bank,
            json!({"media":{"sha256":"test"}}),
            &AtomicBool::new(false),
        )?;
        let mut archive = zip::ZipArchive::new(Cursor::new(data))?;
        assert_eq!(archive.len(), 3);
        let mut text = String::new();
        archive
            .by_name("instruments/bank-000-program-007.sfz")?
            .read_to_string(&mut text)?;
        for expected in [
            "pitch_keycenter=48",
            "pitch_keytrack=0",
            "transpose=-3 tune=83",
            "pan=-50.000000",
            "ampeg_sustain=10.000000",
            "ampeg_release=0.800000",
            "loop_mode=loop_continuous loop_start=1 loop_end=3",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        let mut wave = Vec::new();
        archive
            .by_name("samples/sample-0000.wav")?
            .read_to_end(&mut wave)?;
        let mut reader = hound::WavReader::new(Cursor::new(wave))?;
        assert_eq!(reader.duration(), 4);
        assert_eq!(
            reader
                .samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?,
            pcm
        );
        let mut manifest = Vec::new();
        archive
            .by_name("manifest.json")?
            .read_to_end(&mut manifest)?;
        let metadata: Value = serde_json::from_slice(&manifest)?;
        assert_eq!(metadata["media"]["sha256"], "test");
        assert_eq!(metadata["programs"][0]["program"], 7);
        assert_eq!(bank.samples[0].pcm, pcm);
        Ok(())
    }
}

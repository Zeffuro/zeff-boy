use anyhow::{Result, ensure};

use super::bank::{InstrumentBank, Region, Sample};

const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const FIXED_ONE: f64 = 65_536.0;
const KEY_NUMBER: u16 = 0x0003;
const PITCH: u16 = 0x0003;
const PAN: u16 = 0x0004;
const EG1_ATTACK: u16 = 0x0206;
const EG1_DECAY: u16 = 0x0207;
const EG1_RELEASE: u16 = 0x0209;
const EG1_SUSTAIN: u16 = 0x020a;

pub(super) fn encode(bank: &InstrumentBank) -> Result<Vec<u8>> {
    super::bank::validate(bank)?;
    let mut out = Riff::default();
    let collection = out.begin(b"RIFF")?;
    out.append(b"DLS ")?;
    out.chunk(b"colh", &(bank.presets.len() as u32).to_le_bytes())?;

    let instruments = out.list(b"lins")?;
    for preset in &bank.presets {
        let instrument = out.list(b"ins ")?;
        let header = out.begin(b"insh")?;
        out.u32(preset.zones.len() as u32)?;
        let bank_number = if preset.bank == 128 {
            0x8000_0000
        } else {
            (u32::from(preset.bank >> 7) << 8) | u32::from(preset.bank & 127)
        };
        out.u32(bank_number)?;
        out.u32(u32::from(preset.program))?;
        out.end(header)?;
        let regions = out.list(b"lrgn")?;
        for region in &preset.zones {
            write_region(&mut out, region, &bank.samples[region.sample_index])?;
        }
        out.end(regions)?;
        out.info(&preset.name, None)?;
        out.end(instrument)?;
    }
    out.end(instruments)?;

    let pool = out.begin(b"ptbl")?;
    out.u32(8)?; // POOLTABLE header size, excluding cue records.
    out.u32(bank.samples.len() as u32)?;
    let cue_positions = out.bytes.len();
    for _ in &bank.samples {
        out.u32(0)?;
    }
    out.end(pool)?;
    let waves = out.list(b"wvpl")?;
    let wave_base = out.bytes.len();
    for (index, sample) in bank.samples.iter().enumerate() {
        // Cues are relative to the first byte after the wvpl list type.
        let relative = u32::try_from(out.bytes.len() - wave_base)?;
        out.bytes[cue_positions + index * 4..cue_positions + index * 4 + 4]
            .copy_from_slice(&relative.to_le_bytes());
        let wave = out.list(b"wave")?;
        let format = out.begin(b"fmt ")?;
        out.u16(1)?; // WAVE_FORMAT_PCM
        out.u16(1)?; // Mono
        out.u32(sample.integer_rate())?;
        out.u32(sample.integer_rate() * 2)?;
        out.u16(2)?;
        out.u16(16)?;
        out.end(format)?;
        let data = out.begin(b"data")?;
        out.pcm(&sample.pcm)?;
        out.end(data)?;
        write_wsmp(
            &mut out,
            sample,
            sample.original_pitch,
            rate_correction(sample)?,
        )?;
        out.info(&sample.name, None)?;
        out.end(wave)?;
    }
    out.end(waves)?;
    out.info(&bank.name, Some(&bank.comment))?;
    out.end(collection)?;
    Ok(out.bytes)
}

fn write_region(out: &mut Riff, region: &Region, sample: &Sample) -> Result<()> {
    ensure!(
        region.scale_tuning <= 100,
        "DLS2 supports key tracking from 0 to 100 cents per key"
    );
    let start = out.list(b"rgn2")?;
    let header = out.begin(b"rgnh")?;
    out.u16(u16::from(region.key_start))?;
    out.u16(u16::from(region.key_end))?;
    out.u16(0)?;
    out.u16(127)?;
    out.u16(1)?; // Self-nonexclusive: repeated notes may overlap.
    out.u16(0)?; // No exclusive key group; the optional layer field is absent.
    out.end(header)?;

    // DLS subtracts 100 * unity note independently of key tracking. Compensate
    // that subtraction to preserve (key - root) * scale, including fixed pitch.
    let tuning = i32::from(rate_correction(sample)?)
        + i32::from(region.coarse_tune) * 100
        + i32::from(region.fine_tune)
        + (100 - i32::from(region.scale_tuning)) * i32::from(region.root_key);
    let tuning = i16::try_from(tuning)
        .map_err(|_| anyhow::anyhow!("DLS sample fine tuning is out of range"))?;
    write_wsmp(out, sample, region.root_key, tuning)?;

    let link = out.begin(b"wlnk")?;
    out.u16(0)?; // Articulation controls pan; no fixed multichannel steering.
    out.u16(0)?;
    out.u32(1)?; // Mono
    out.u32(region.sample_index as u32)?;
    out.end(link)?;

    let articulation = out.list(b"lar2")?;
    let connections = out.begin(b"art2")?;
    out.u32(8)?;
    out.u32(6)?;
    connection(
        out,
        KEY_NUMBER,
        PITCH,
        i32::from(region.scale_tuning) * 128 * 65_536,
    )?;
    connection(out, 0, PAN, i32::from(region.pan) * 65_536)?;
    connection(
        out,
        0,
        EG1_ATTACK,
        timecents(f64::from(region.attack_seconds))?,
    )?;
    connection(
        out,
        0,
        EG1_DECAY,
        timecents(decay_full_scale_seconds(region))?,
    )?;
    connection(out, 0, EG1_SUSTAIN, sustain_gain(region.sustain_level))?;
    // Neutral release is the full-scale-to--100 dB rate. DLS uses -96 dB.
    connection(
        out,
        0,
        EG1_RELEASE,
        timecents(f64::from(region.release_seconds) * 0.96)?,
    )?;
    out.end(connections)?;
    out.end(articulation)?;
    out.end(start)
}

fn write_wsmp(out: &mut Riff, sample: &Sample, root: u8, tuning: i16) -> Result<()> {
    let chunk = out.begin(b"wsmp")?;
    out.u32(20)?;
    out.u16(u16::from(root))?;
    out.append(&tuning.to_le_bytes())?; // Signed integer cents, positive raises pitch.
    out.u32(0)?; // No attenuation.
    out.u32(3)?; // Request no truncation or lossy compression.
    out.u32(u32::from(sample.loop_range.is_some()))?;
    if let Some((start, end)) = sample.loop_range {
        out.u32(16)?;
        out.u32(0)?; // Forward loop continues during envelope release.
        out.u32(start)?;
        out.u32(end - start)?;
    }
    out.end(chunk)
}

fn rate_correction(sample: &Sample) -> Result<i16> {
    Ok(sample.pitch_correction())
}

fn connection(out: &mut Riff, source: u16, destination: u16, scale: i32) -> Result<()> {
    out.u16(source)?;
    out.u16(0)?; // No control source.
    out.u16(destination)?;
    out.u16(0)?; // Positive unipolar, linear transforms.
    out.append(&scale.to_le_bytes())
}

fn timecents(seconds: f64) -> Result<i32> {
    if seconds == 0.0 {
        return Ok(i32::MIN);
    }
    let value = (1200.0 * seconds.log2() * FIXED_ONE).round();
    ensure!(
        value.is_finite() && value > f64::from(i32::MIN) && value <= f64::from(i32::MAX),
        "DLS envelope time is out of range"
    );
    Ok(value as i32)
}

fn decay_full_scale_seconds(region: &Region) -> f64 {
    if region.sustain_level == 1.0 {
        return 0.0;
    }
    // Neutral decay lasts until sustain (or -100 dB when sustain is zero).
    let attenuation = (-20.0 * f64::from(region.sustain_level).max(0.00001).log10()).min(100.0);
    f64::from(region.decay_seconds) * 96.0 / attenuation
}

fn sustain_gain(amplitude: f32) -> i32 {
    if amplitude == 0.0 {
        return 0;
    }
    // DLS sustain is a fraction of the -96 dB..0 dB gain envelope, not amplitude.
    ((1.0 + 20.0 * f64::from(amplitude).log10() / 96.0).clamp(0.0, 1.0) * 1000.0 * FIXED_ONE)
        .round() as i32
}

#[derive(Default)]
struct Riff {
    bytes: Vec<u8>,
}

impl Riff {
    fn reserve(&mut self, count: usize) -> Result<()> {
        ensure!(
            self.bytes
                .len()
                .checked_add(count)
                .is_some_and(|size| size <= MAX_OUTPUT_BYTES),
            "DLS output exceeds 128 MiB"
        );
        self.bytes.try_reserve(count)?;
        Ok(())
    }

    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.reserve(bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn u16(&mut self, value: u16) -> Result<()> {
        self.append(&value.to_le_bytes())
    }

    fn u32(&mut self, value: u32) -> Result<()> {
        self.append(&value.to_le_bytes())
    }

    fn begin(&mut self, id: &[u8; 4]) -> Result<usize> {
        let start = self.bytes.len();
        self.append(id)?;
        self.u32(0)?;
        Ok(start)
    }

    fn list(&mut self, kind: &[u8; 4]) -> Result<usize> {
        let start = self.begin(b"LIST")?;
        self.append(kind)?;
        Ok(start)
    }

    fn end(&mut self, start: usize) -> Result<()> {
        let size = self.bytes.len() - start - 8;
        self.bytes[start + 4..start + 8].copy_from_slice(&u32::try_from(size)?.to_le_bytes());
        if size & 1 != 0 {
            self.append(&[0])?;
        }
        Ok(())
    }

    fn chunk(&mut self, id: &[u8; 4], bytes: &[u8]) -> Result<()> {
        let start = self.begin(id)?;
        self.append(bytes)?;
        self.end(start)
    }

    fn pcm(&mut self, pcm: &[i16]) -> Result<()> {
        self.reserve(
            pcm.len()
                .checked_mul(2)
                .ok_or_else(|| anyhow::anyhow!("DLS PCM size overflow"))?,
        )?;
        for point in pcm {
            self.bytes.extend_from_slice(&point.to_le_bytes());
        }
        Ok(())
    }

    fn text(&mut self, id: &[u8; 4], value: &str) -> Result<()> {
        ensure!(
            value.len() <= MAX_TEXT_BYTES && !value.contains('\0'),
            "invalid DLS information text"
        );
        let start = self.begin(id)?;
        self.append(value.as_bytes())?;
        self.append(&[0])?;
        self.end(start)
    }

    fn info(&mut self, name: &str, comment: Option<&str>) -> Result<()> {
        let start = self.list(b"INFO")?;
        self.text(b"INAM", name)?;
        if let Some(comment) = comment {
            self.text(b"ICMT", comment)?;
            self.text(b"ISFT", "zeff-boy")?;
        }
        self.end(start)
    }
}

#[cfg(test)]
mod tests {
    use super::super::bank::Preset;
    use super::*;

    fn fixture() -> InstrumentBank {
        let zone = Region {
            key_start: 0,
            key_end: 127,
            sample_index: 0,
            root_key: 60,
            scale_tuning: 100,
            coarse_tune: 0,
            fine_tune: 7,
            pan: -250,
            attack_seconds: 0.0,
            decay_seconds: 0.5,
            sustain_level: 0.5,
            release_seconds: 1.0,
        };
        InstrumentBank {
            name: "Test".into(),
            comment: "odd!".into(),
            samples: vec![
                Sample {
                    name: "first".into(),
                    pcm: vec![-32768, -3, 17, 32767],
                    sample_rate: 32_000.25,
                    original_pitch: 60,
                    loop_range: Some((1, 4)),
                },
                Sample {
                    name: "second".into(),
                    pcm: vec![7, 11, -17],
                    sample_rate: 22_050.0,
                    original_pitch: 65,
                    loop_range: None,
                },
            ],
            presets: vec![
                Preset {
                    name: "Melodic".into(),
                    bank: 0,
                    program: 12,
                    zones: vec![
                        zone.clone(),
                        Region {
                            sample_index: 1,
                            root_key: 65,
                            pan: 500,
                            ..zone.clone()
                        },
                    ],
                },
                Preset {
                    name: "Drums".into(),
                    bank: 128,
                    program: 12,
                    zones: vec![Region {
                        scale_tuning: 0,
                        coarse_tune: -12,
                        fine_tune: 25,
                        ..zone
                    }],
                },
            ],
        }
    }

    fn u16_at(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn chunks(mut bytes: &[u8]) -> Vec<(&[u8], &[u8])> {
        let mut result = Vec::new();
        while !bytes.is_empty() {
            assert!(bytes.len() >= 8);
            let size = u32_at(bytes, 4) as usize;
            assert!(size <= bytes.len() - 8);
            result.push((&bytes[..4], &bytes[8..8 + size]));
            let padded = size + (size & 1);
            if padded != size {
                assert_eq!(bytes[8 + size], 0);
            }
            bytes = &bytes[8 + padded..];
        }
        result
    }

    fn child<'a>(bytes: &'a [u8], id: &[u8]) -> &'a [u8] {
        chunks(bytes)
            .into_iter()
            .find(|(kind, _)| *kind == id)
            .unwrap()
            .1
    }

    fn list<'a>(bytes: &'a [u8], kind: &[u8]) -> &'a [u8] {
        let value = chunks(bytes)
            .into_iter()
            .find(|(id, value)| *id == b"LIST" && &value[..4] == kind)
            .unwrap()
            .1;
        &value[4..]
    }

    fn scale(region: &[u8], source: u16, destination: u16) -> i32 {
        let art = child(list(region, b"lar2"), b"art2");
        assert_eq!(u32_at(art, 0), 8);
        assert_eq!(art.len(), 8 + u32_at(art, 4) as usize * 12);
        let block = art[8..]
            .as_chunks::<12>()
            .0
            .iter()
            .find(|block| u16_at(*block, 0) == source && u16_at(*block, 4) == destination)
            .unwrap();
        assert_eq!(u16_at(block, 2), 0);
        assert_eq!(u16_at(block, 6), 0);
        i32::from_le_bytes(block[8..12].try_into().unwrap())
    }

    #[test]
    fn dls2_chunks_preserve_pcm_cues_loops_and_per_region_articulation() {
        let bank = fixture();
        let encoded = encode(&bank).unwrap();
        assert_eq!(&encoded[..4], b"RIFF");
        assert_eq!(u32_at(&encoded, 4) as usize + 8, encoded.len());
        assert_eq!(&encoded[8..12], b"DLS ");
        let body = &encoded[12..];
        assert_eq!(u32_at(child(body, b"colh"), 0), 2);
        let instruments = chunks(list(body, b"lins"));
        let melodic = &instruments[0].1[4..];
        let drum = &instruments[1].1[4..];
        assert_eq!(u32_at(child(melodic, b"insh"), 0), 2);
        assert_eq!(u32_at(child(drum, b"insh"), 4), 0x8000_0000);
        assert_eq!(u32_at(child(drum, b"insh"), 8), 12);
        let regions = chunks(list(melodic, b"lrgn"));
        assert_eq!(&regions[0].1[..4], b"rgn2");
        let first = &regions[0].1[4..];
        let second = &regions[1].1[4..];
        assert_eq!(scale(first, 0, PAN), -250 * 65_536);
        assert_eq!(scale(second, 0, PAN), 500 * 65_536);
        assert_eq!(scale(first, 0, EG1_ATTACK), i32::MIN);
        assert_eq!(scale(first, KEY_NUMBER, PITCH), 838_860_800);
        let sustain = f64::from(scale(first, 0, EG1_SUSTAIN)) / 65_536_000.0;
        assert!((10_f64.powf((sustain - 1.0) * 96.0 / 20.0) - 0.5).abs() < 0.000001);
        let decay = 2_f64.powf(f64::from(scale(first, 0, EG1_DECAY)) / (1200.0 * FIXED_ONE));
        assert!((decay * (-20.0 * 0.5_f64.log10()) / 96.0 - 0.5).abs() < 0.000001);
        let release = 2_f64.powf(f64::from(scale(first, 0, EG1_RELEASE)) / (1200.0 * FIXED_ONE));
        assert!((release - 0.96).abs() < 0.000001);
        let wsmp = child(first, b"wsmp");
        assert_eq!(u32_at(wsmp, 0), 20);
        assert_eq!(u32_at(wsmp, 16), 1);
        assert_eq!(u32_at(wsmp, 20), 16);
        assert_eq!(u32_at(wsmp, 24), 0);
        assert_eq!(u32_at(wsmp, 28), 1);
        assert_eq!(u32_at(wsmp, 32), 3);
        assert_eq!(u32_at(child(second, b"wsmp"), 16), 0);
        let pool = child(body, b"ptbl");
        assert_eq!(u32_at(pool, 0), 8);
        assert_eq!(u32_at(pool, 4), 2);
        let waves = list(body, b"wvpl");
        for (index, sample) in bank.samples.iter().enumerate() {
            let cue = u32_at(pool, 8 + index * 4) as usize;
            assert_eq!(&waves[cue..cue + 4], b"LIST");
            assert_eq!(&waves[cue + 8..cue + 12], b"wave");
            let wave = &waves[cue + 12..cue + 8 + u32_at(waves, cue + 4) as usize];
            assert_eq!(u32_at(child(wave, b"fmt "), 4), sample.integer_rate());
            let points = child(wave, b"data")
                .as_chunks::<2>()
                .0
                .iter()
                .map(|bytes| i16::from_le_bytes(*bytes))
                .collect::<Vec<_>>();
            assert_eq!(points, sample.pcm);
        }
    }

    #[test]
    fn fixed_pitch_compensates_unity_note_and_keeps_tuning_sign() {
        let mut bank = fixture();
        // A low exact rate deliberately needs more than an i8 correction.
        bank.samples[0].sample_rate = 1.4;
        let encoded = encode(&bank).unwrap();
        let instruments = chunks(list(&encoded[12..], b"lins"));
        let regions = chunks(list(&instruments[1].1[4..], b"lrgn"));
        let region = &regions[0].1[4..];
        let wsmp = child(region, b"wsmp");
        let root = f64::from(u16_at(wsmp, 4));
        let fine = f64::from(i16::from_le_bytes(wsmp[6..8].try_into().unwrap()));
        let key_scale = f64::from(scale(region, KEY_NUMBER, PITCH)) / FIXED_ONE;
        for key in [0, 60, 127] {
            let cents = f64::from(key) / 128.0 * key_scale - root * 100.0 + fine;
            let expected = -1200.0 + 25.0 + (1200.0 * 1.4_f64.log2()).round();
            assert_eq!(cents, expected);
        }
        assert_eq!(key_scale, 0.0);
        assert!(fine > 4825.0);
    }

    #[test]
    fn unsupported_tracking_and_invalid_banks_fail_without_clamping() {
        let mut bank = fixture();
        bank.presets[0].zones[0].scale_tuning = 101;
        assert!(
            encode(&bank)
                .unwrap_err()
                .to_string()
                .contains("key tracking")
        );
        bank.presets[0].zones[0].scale_tuning = 100;
        bank.samples[0].loop_range = Some((4, 4));
        assert!(encode(&bank).is_err());
        assert_eq!(sustain_gain(0.0), 0);
        assert_eq!(sustain_gain(1.0), 65_536_000);
        assert_eq!(timecents(1.0).unwrap(), 0);
        assert_eq!(timecents(0.5).unwrap(), -78_643_200);
        let mut writer = Riff::default();
        assert!(writer.reserve(MAX_OUTPUT_BYTES + 1).is_err());
        assert!(writer.reserve(usize::MAX).is_err());
        assert!(writer.bytes.is_empty());
    }
}

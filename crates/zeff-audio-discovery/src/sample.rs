use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde::Serialize;

use super::{EngineProfile, MAX_ROM_BYTES, RomSpan, SampleInventory};

const HEADER_LEN: usize = 16;
const BDPCM_BLOCK_SAMPLES: usize = 64;
const BDPCM_BLOCK_BYTES: usize = 33;
const BDPCM_DELTAS: [i8; 16] = [
    0, 1, 4, 9, 16, 25, 36, 49, -64, -49, -36, -25, -16, -9, -4, -1,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleEncoding {
    PcmS8,
    GameFreakBdpcm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleDirection {
    Forward,
    Reverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleError {
    Invalid,
    Unsupported,
    Empty,
}

impl fmt::Display for SampleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "invalid sample",
            Self::Unsupported => "unsupported sample encoding",
            Self::Empty => "empty sample",
        })
    }
}

impl std::error::Error for SampleError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedSample {
    pub pcm: Vec<i16>,
    /// Zero-based, half-open decoded PCM indices.
    pub loop_range: Option<(u32, u32)>,
}

pub(crate) fn read_sample_inventory(
    bytes: &[u8],
    header_offset: usize,
    kind: u8,
    profile: EngineProfile,
) -> Result<SampleInventory, SampleError> {
    let header = bytes
        .get(
            header_offset
                ..header_offset
                    .checked_add(HEADER_LEN)
                    .ok_or(SampleError::Invalid)?,
        )
        .ok_or(SampleError::Invalid)?;
    let header_mode = u16::from_le_bytes([header[0], header[1]]);
    let loop_flag = u16::from_le_bytes([header[2], header[3]]);
    if !matches!(loop_flag, 0 | 0x4000) {
        return Err(SampleError::Unsupported);
    }

    let (encoding, direction) = interpretation(profile, kind, header_mode)?;
    let frequency = u32::from_le_bytes(header[4..8].try_into().expect("four header bytes"));
    let loop_start = u32::from_le_bytes(header[8..12].try_into().expect("four header bytes"));
    let decoded_len = u32::from_le_bytes(header[12..16].try_into().expect("four header bytes"));
    let decoded_count = usize::try_from(decoded_len).map_err(|_| SampleError::Invalid)?;
    if decoded_count == 0 {
        return Err(SampleError::Empty);
    }
    if decoded_count > MAX_ROM_BYTES || frequency == 0 {
        return Err(SampleError::Invalid);
    }
    let looped = loop_flag == 0x4000;
    if looped && loop_start >= decoded_len {
        return Err(SampleError::Invalid);
    }

    let raw_len = encoded_len(encoding, decoded_count).ok_or(SampleError::Invalid)?;
    let data_offset = header_offset
        .checked_add(HEADER_LEN)
        .ok_or(SampleError::Invalid)?;
    bytes
        .get(
            data_offset
                ..data_offset
                    .checked_add(raw_len)
                    .ok_or(SampleError::Invalid)?,
        )
        .ok_or(SampleError::Invalid)?;

    Ok(SampleInventory {
        header: RomSpan::new(header_offset, HEADER_LEN),
        data: RomSpan::new(data_offset, raw_len),
        frequency,
        loop_start,
        looped,
        decoded_len,
        encoding,
        direction,
    })
}

fn interpretation(
    profile: EngineProfile,
    kind: u8,
    header_mode: u16,
) -> Result<(SampleEncoding, SampleDirection), SampleError> {
    use SampleDirection::{Forward, Reverse};
    use SampleEncoding::{GameFreakBdpcm, PcmS8};

    match (profile, kind, header_mode) {
        (EngineProfile::Mp2k, 0x00 | 0x08, 0) => Ok((PcmS8, Forward)),
        (EngineProfile::Mp2k, 0x10 | 0x18, 0) => Ok((PcmS8, Reverse)),
        (EngineProfile::Mp2k, 0x20 | 0x28, 1) => Ok((GameFreakBdpcm, Forward)),
        (EngineProfile::Mp2k, 0x30 | 0x38, 1) => Ok((GameFreakBdpcm, Reverse)),
        (EngineProfile::Mp2kSongId, 0x00 | 0x08, 0) => Ok((PcmS8, Forward)),
        (EngineProfile::Mp2kSongId, 0x20, 0) => Ok((PcmS8, Reverse)),
        (EngineProfile::Mp2k, 0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38, _)
        | (EngineProfile::Mp2kSongId, 0x00 | 0x08 | 0x20, _) => Err(SampleError::Unsupported),
        _ => Err(SampleError::Unsupported),
    }
}

fn encoded_len(encoding: SampleEncoding, decoded_count: usize) -> Option<usize> {
    match encoding {
        SampleEncoding::PcmS8 => Some(decoded_count),
        SampleEncoding::GameFreakBdpcm => decoded_count
            .checked_add(BDPCM_BLOCK_SAMPLES - 1)?
            .checked_div(BDPCM_BLOCK_SAMPLES)?
            .checked_mul(BDPCM_BLOCK_BYTES),
    }
}

pub fn decode_sample(
    bytes: &[u8],
    sample: &SampleInventory,
    cancel: &AtomicBool,
) -> Result<DecodedSample> {
    let decoded_count = usize::try_from(sample.decoded_len)?;
    ensure!(decoded_count != 0, "sample has no decoded PCM points");
    ensure!(
        decoded_count <= MAX_ROM_BYTES,
        "sample exceeds the decoded PCM point limit"
    );
    let expected_raw_len = encoded_len(sample.encoding, decoded_count)
        .ok_or_else(|| anyhow::anyhow!("sample encoded length overflows"))?;
    ensure!(
        sample.data.byte_len as usize == expected_raw_len,
        "sample raw span does not match its encoding and decoded length"
    );
    let start = sample.data.effective_offset as usize;
    ensure!(
        sample.data.canonical_cpu_address
            == 0x0800_0000_u32
                .checked_add(sample.data.effective_offset)
                .ok_or_else(|| anyhow::anyhow!("sample ROM address overflows"))?,
        "sample raw span has an invalid ROM mapping"
    );
    let raw = bytes
        .get(
            start
                ..start
                    .checked_add(expected_raw_len)
                    .ok_or_else(|| anyhow::anyhow!("sample raw span overflows"))?,
        )
        .ok_or_else(|| anyhow::anyhow!("sample raw span is outside the ROM"))?;

    let mut pcm = Vec::with_capacity(decoded_count);
    match sample.encoding {
        SampleEncoding::PcmS8 => {
            for block in raw.chunks(BDPCM_BLOCK_SAMPLES * 1024) {
                ensure!(!cancel.load(Ordering::Relaxed), "sample decoding cancelled");
                pcm.extend(block.iter().map(|byte| i16::from(*byte as i8) * 256));
            }
        }
        SampleEncoding::GameFreakBdpcm => {
            for block in raw.as_chunks::<BDPCM_BLOCK_BYTES>().0 {
                ensure!(!cancel.load(Ordering::Relaxed), "sample decoding cancelled");
                decode_bdpcm_block(block, &mut pcm);
            }
            pcm.truncate(decoded_count);
        }
    }
    ensure!(
        pcm.len() == decoded_count,
        "sample decoder produced the wrong PCM point count"
    );
    if sample.direction == SampleDirection::Reverse {
        pcm.reverse();
    }

    let loop_range = if sample.direction == SampleDirection::Forward && sample.looped {
        ensure!(
            sample.loop_start < sample.decoded_len,
            "sample loop starts outside decoded PCM"
        );
        Some((sample.loop_start, sample.decoded_len))
    } else {
        None
    };
    Ok(DecodedSample { pcm, loop_range })
}

fn decode_bdpcm_block(block: &[u8], output: &mut Vec<i16>) {
    debug_assert_eq!(block.len(), BDPCM_BLOCK_BYTES);
    let mut accumulator = block[0] as i8;
    output.push(i16::from(accumulator) * 256);

    accumulator = accumulator.wrapping_add(BDPCM_DELTAS[usize::from(block[1] & 0x0f)]);
    output.push(i16::from(accumulator) * 256);
    for packed in &block[2..] {
        accumulator = accumulator.wrapping_add(BDPCM_DELTAS[usize::from(packed >> 4)]);
        output.push(i16::from(accumulator) * 256);
        accumulator = accumulator.wrapping_add(BDPCM_DELTAS[usize::from(packed & 0x0f)]);
        output.push(i16::from(accumulator) * 256);
    }
    debug_assert_eq!(output.len() % BDPCM_BLOCK_SAMPLES, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(mode: u16, looped: bool, loop_start: u32, len: u32, data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; HEADER_LEN + data.len()];
        bytes[0..2].copy_from_slice(&mode.to_le_bytes());
        bytes[2..4].copy_from_slice(&(if looped { 0x4000u16 } else { 0 }).to_le_bytes());
        bytes[4..8].copy_from_slice(&(8_000 * 1024u32).to_le_bytes());
        bytes[8..12].copy_from_slice(&loop_start.to_le_bytes());
        bytes[12..16].copy_from_slice(&len.to_le_bytes());
        bytes[16..].copy_from_slice(data);
        bytes
    }

    #[test]
    fn profile_controls_kind_20_and_reverse_disables_looping() {
        let bytes = header(0, true, 1, 4, &[0, 1, 0x80, 0xff]);
        let song_id = read_sample_inventory(&bytes, 0, 0x20, EngineProfile::Mp2kSongId).unwrap();
        assert_eq!(song_id.encoding, SampleEncoding::PcmS8);
        assert_eq!(song_id.direction, SampleDirection::Reverse);
        assert!(song_id.looped);
        let decoded = decode_sample(&bytes, &song_id, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.pcm, [-256, -32_768, 256, 0]);
        assert_eq!(decoded.loop_range, None);

        assert_eq!(
            read_sample_inventory(&bytes, 0, 0x20, EngineProfile::Mp2k),
            Err(SampleError::Unsupported)
        );
    }

    #[test]
    fn mp2k_reverse_pcm_reverses_every_decoded_point() {
        let bytes = header(0, false, 0, 5, &[1, 2, 3, 4, 5]);
        let sample = read_sample_inventory(&bytes, 0, 0x10, EngineProfile::Mp2k).unwrap();
        assert_eq!(sample.data.byte_len, 5);
        let decoded = decode_sample(&bytes, &sample, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.pcm, [1280, 1024, 768, 512, 256]);
    }

    #[test]
    fn bdpcm_uses_33_raw_bytes_per_64_decoded_points_and_exact_nibble_order() {
        let mut block = [0u8; BDPCM_BLOCK_BYTES];
        block[0] = 120;
        block[1] = 0xa7; // The high nibble is padding and is ignored.
        block[2] = 0x81; // High nibble first from byte two onward.
        let bytes = header(1, true, 2, 4, &block);
        let sample = read_sample_inventory(&bytes, 0, 0x20, EngineProfile::Mp2k).unwrap();
        assert_eq!(sample.data.byte_len, 33);
        assert_eq!(sample.decoded_len, 4);
        let decoded = decode_sample(&bytes, &sample, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.pcm, [120 * 256, -87 * 256, 105 * 256, 106 * 256]);
        assert_eq!(decoded.loop_range, Some((2, 4)));
    }

    #[test]
    fn bdpcm_wraps_signed_accumulator_and_reverse_operates_after_decode() {
        let mut block = [0u8; BDPCM_BLOCK_BYTES];
        block[0] = 127;
        block[1] = 1;
        block[2] = 0x11;
        let bytes = header(1, false, 0, 4, &block);
        let sample = read_sample_inventory(&bytes, 0, 0x30, EngineProfile::Mp2k).unwrap();
        let decoded = decode_sample(&bytes, &sample, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.pcm, [-126 * 256, -127 * 256, -128 * 256, 127 * 256]);
    }

    #[test]
    fn malformed_headers_lengths_and_cancellation_fail_explicitly() {
        let empty = header(0, false, 0, 0, &[]);
        assert_eq!(
            read_sample_inventory(&empty, 0, 0, EngineProfile::Mp2k),
            Err(SampleError::Empty)
        );
        let short = header(1, false, 0, 65, &[0; 33]);
        assert_eq!(
            read_sample_inventory(&short, 0, 0x20, EngineProfile::Mp2k),
            Err(SampleError::Invalid)
        );
        let bytes = header(0, false, 0, 1, &[7]);
        let sample = read_sample_inventory(&bytes, 0, 0, EngineProfile::Mp2k).unwrap();
        assert!(decode_sample(&bytes, &sample, &AtomicBool::new(true)).is_err());
    }
}

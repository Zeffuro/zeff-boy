use std::io::{Cursor, Read, Seek, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use super::formats::AudioFormat;
use anyhow::{Context, Result, ensure};

const MAX_ARTIFACT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 1024 * 1024;

pub(super) struct AudioData<'a> {
    pub(super) pcm: &'a [i16],
    pub(super) channels: u16,
    pub(super) sample_rate: u32,
    pub(super) loop_range: Option<(u32, u32)>,
    pub(super) pitch: Option<(u8, i16)>,
}

#[derive(Clone, Copy)]
pub(super) struct AudioInfo {
    pub(super) frames: u64,
    pub(super) channels: u16,
    pub(super) sample_rate: u32,
    pub(super) loop_range: Option<(u32, u32)>,
    pub(super) pitch: Option<(u8, i16)>,
}

impl AudioInfo {
    pub(super) fn validate(self) -> Result<()> {
        ensure!(
            matches!(self.channels, 1 | 2) && (1..=192_000).contains(&self.sample_rate),
            "invalid audio channel count or sample rate"
        );
        ensure!(
            self.frames > 0
                && self.frames
                    <= (MAX_ARTIFACT_BYTES - MAX_METADATA_BYTES as u64)
                        / (u64::from(self.channels) * 2),
            "audio exceeds its PCM size limit"
        );
        if let Some((start, end)) = self.loop_range {
            ensure!(
                start < end && u64::from(end) <= self.frames,
                "invalid decoded sample loop"
            );
        }
        ensure!(
            self.pitch.is_none_or(|(root, _)| root <= 127),
            "invalid sample root key"
        );
        Ok(())
    }
}

pub(super) fn encode(
    format: AudioFormat,
    audio: AudioData<'_>,
    metadata: &[u8],
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    ensure!(
        matches!(audio.channels, 1 | 2) && audio.pcm.len().is_multiple_of(audio.channels as usize),
        "invalid interleaved PCM length"
    );
    ensure!(
        audio.pcm.len() <= 64 * 1024 * 1024,
        "in-memory audio exceeds its size limit"
    );
    let info = AudioInfo {
        frames: (audio.pcm.len() / audio.channels as usize) as u64,
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        loop_range: audio.loop_range,
        pitch: audio.pitch,
    };
    let mut input = Cursor::new(
        audio
            .pcm
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    let mut output = Cursor::new(Vec::new());
    encode_to(format, &mut input, info, metadata, cancel, &mut output)?;
    Ok(output.into_inner())
}

pub(super) fn encode_to(
    format: AudioFormat,
    input: &mut (impl Read + Seek),
    audio: AudioInfo,
    metadata: &[u8],
    cancel: &AtomicBool,
    output: &mut (impl Write + Seek),
) -> Result<()> {
    audio.validate()?;
    check_cancel(cancel)?;
    ensure!(
        metadata.len() <= MAX_METADATA_BYTES,
        "audio metadata exceeds its size limit"
    );
    input.rewind()?;
    match format {
        AudioFormat::Wav => {
            output.write_all(&wav_header(audio, metadata)?)?;
            let mut remaining = audio.frames * u64::from(audio.channels) * 2;
            let mut block = [0u8; 64 * 1024];
            while remaining > 0 {
                check_cancel(cancel)?;
                let count = remaining.min(block.len() as u64) as usize;
                input.read_exact(&mut block[..count])?;
                output.write_all(&block[..count])?;
                remaining -= count as u64;
            }
        }
        AudioFormat::Flac => flac(input, audio, metadata, cancel, output)?,
        AudioFormat::Ogg => ogg(input, audio, metadata, cancel, output)?,
    }
    ensure!(
        input.read(&mut [0])? == 0,
        "PCM source contains unexpected trailing data"
    );
    ensure!(
        output.stream_position()? <= MAX_ARTIFACT_BYTES,
        "encoded audio exceeds its size limit"
    );
    check_cancel(cancel)
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "export cancelled");
    Ok(())
}

fn comments(audio: AudioInfo, metadata: &[u8]) -> Result<Vec<u8>> {
    let metadata = std::str::from_utf8(metadata).context("audio metadata must be UTF-8")?;
    let mut values = vec![
        format!("ZEFF_METADATA={metadata}"),
        "ENCODER=zeff-boy".to_owned(),
    ];
    if let Some((start, end)) = audio.loop_range {
        values.push(format!("LOOPSTART={start}"));
        values.push(format!("LOOPEND={end}"));
    }
    if let Some((root, correction)) = audio.pitch {
        values.push(format!("MIDI_UNITY_NOTE={root}"));
        values.push(format!("PITCH_CORRECTION_CENTS={correction}"));
    }
    let mut output = Vec::new();
    let vendor = b"zeff-boy";
    output.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    output.extend_from_slice(vendor);
    output.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        output.extend_from_slice(&(value.len() as u32).to_le_bytes());
        output.extend_from_slice(value.as_bytes());
    }
    Ok(output)
}

fn pcm_block(input: &mut impl Read, count: usize) -> Result<Vec<i16>> {
    let mut bytes = vec![0; count * 2];
    input.read_exact(&mut bytes)?;
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect())
}

fn flac(
    input: &mut impl Read,
    audio: AudioInfo,
    metadata: &[u8],
    cancel: &AtomicBool,
    output: &mut (impl Write + Seek),
) -> Result<()> {
    use flacenc::component::{BitRepr, MetadataBlockData, Stream, StreamInfo};
    use flacenc::error::Verify;
    use flacenc::source::{Context as DigestContext, Fill, FrameBuf};
    fn error(error: impl std::fmt::Display) -> anyhow::Error {
        anyhow::anyhow!("FLAC encoding failed: {error}")
    }
    fn header(info: StreamInfo, comment: &[u8]) -> Result<Vec<u8>> {
        let mut stream = Stream::with_stream_info(info);
        stream.add_metadata_block(MetadataBlockData::new_unknown(4, comment).map_err(error)?);
        let mut sink = flacenc::bitsink::ByteSink::new();
        stream.write(&mut sink).map_err(error)?;
        Ok(sink.as_slice().to_vec())
    }
    const BLOCK: usize = 4096;
    let channels = audio.channels as usize;
    let mut config = flacenc::config::Encoder::default();
    config.subframe_coding.use_lpc = false;
    let config = config.into_verified().map_err(|(_, value)| error(value))?;
    let mut info = StreamInfo::new(audio.sample_rate as usize, channels, 16).map_err(error)?;
    let comment = comments(audio, metadata)?;
    let start = output.stream_position()?;
    let placeholder = header(info.clone(), &comment)?;
    output.write_all(&placeholder)?;
    let mut digest = DigestContext::new(16, channels);
    let mut framebuf = FrameBuf::with_size(channels, BLOCK).map_err(error)?;
    let mut remaining = audio.frames as usize;
    let mut number = 0;
    while remaining > 0 {
        check_cancel(cancel)?;
        let count = remaining.min(BLOCK);
        let samples = pcm_block(input, count * channels)?
            .into_iter()
            .map(i32::from)
            .collect::<Vec<_>>();
        framebuf.fill_interleaved(&samples).map_err(error)?;
        digest.fill_interleaved(&samples).map_err(error)?;
        let frame =
            flacenc::encode_fixed_size_frame(&config, &framebuf, number, &info).map_err(error)?;
        info.update_frame_info(&frame);
        let mut sink = flacenc::bitsink::ByteSink::new();
        frame.write(&mut sink).map_err(error)?;
        output.write_all(sink.as_slice())?;
        ensure!(
            output.stream_position()? <= MAX_ARTIFACT_BYTES,
            "FLAC exceeds its size limit"
        );
        number += 1;
        remaining -= count;
    }
    info.set_block_sizes(BLOCK, BLOCK).map_err(error)?;
    info.set_md5_digest(&digest.md5_digest());
    let final_header = header(info, &comment)?;
    ensure!(
        final_header.len() == placeholder.len(),
        "FLAC header size changed"
    );
    let end = output.stream_position()?;
    output.seek(std::io::SeekFrom::Start(start))?;
    output.write_all(&final_header)?;
    output.seek(std::io::SeekFrom::Start(end))?;
    Ok(())
}

#[cfg(feature = "audio-recording")]
fn ogg(
    input: &mut impl Read,
    audio: AudioInfo,
    metadata: &[u8],
    cancel: &AtomicBool,
    output: &mut (impl Write + Seek),
) -> Result<()> {
    ensure!(
        (8000..=192_000).contains(&audio.sample_rate),
        "Vorbis sample rate must be at least 8000 Hz; use WAV or FLAC for this sample"
    );
    let mut encoder =
        vorbis_encoder::Encoder::new(u32::from(audio.channels), u64::from(audio.sample_rate), 0.6)
            .map_err(|error| anyhow::anyhow!("Vorbis encoder initialization failed: {error}"))?;
    let mut comment = b"\x03vorbis".to_vec();
    comment.extend(comments(audio, metadata)?);
    comment.push(1);
    let mut sequence = 0u32;
    let mut first = true;
    let mut headers_remaining = 3;
    let mut first_audio = true;
    let mut emit = |bytes: Vec<u8>| -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let bytes = if first {
            first = false;
            vorbis_comments(&bytes, &comment, cancel)?
        } else {
            bytes
        };
        let mut position = 0;
        while position < bytes.len() {
            check_cancel(cancel)?;
            let page = ogg_page(&bytes, position)?;
            position += page.len();
            let segments = &page[27..27 + page[26] as usize];
            if headers_remaining > 0 {
                let complete = segments.iter().filter(|length| **length < 255).count();
                ensure!(
                    complete <= headers_remaining,
                    "Vorbis audio shares a header page"
                );
                headers_remaining -= complete;
                write_ogg_page(output, page.to_vec(), &mut sequence)?;
            } else if first_audio {
                first_audio = false;
                // Give decoders an absolute granule anchor before a short stream's
                // EOS page. The first Vorbis audio packet produces no PCM.
                let split = segments
                    .iter()
                    .position(|length| *length < 255)
                    .context("Vorbis preroll packet exceeds one Ogg page")?
                    + 1;
                let prefix_bytes = segments[..split]
                    .iter()
                    .map(|length| *length as usize)
                    .sum::<usize>();
                let body = &page[27 + segments.len()..];
                let mut preroll = page[..27].to_vec();
                preroll[5] = 0;
                preroll[6..14].copy_from_slice(&0u64.to_le_bytes());
                preroll[26] = split as u8;
                preroll.extend_from_slice(&segments[..split]);
                preroll.extend_from_slice(&body[..prefix_bytes]);
                write_ogg_page(output, preroll, &mut sequence)?;
                if split < segments.len() {
                    let mut rest = page[..27].to_vec();
                    rest[5] &= 4;
                    if !segments[split..].iter().any(|length| *length < 255) {
                        rest[6..14].copy_from_slice(&u64::MAX.to_le_bytes());
                    }
                    rest[26] = (segments.len() - split) as u8;
                    rest.extend_from_slice(&segments[split..]);
                    rest.extend_from_slice(&body[prefix_bytes..]);
                    write_ogg_page(output, rest, &mut sequence)?;
                } else {
                    ensure!(page[5] & 4 == 0, "Vorbis stream ended before producing PCM");
                }
            } else {
                write_ogg_page(output, page.to_vec(), &mut sequence)?;
            }
        }
        ensure!(
            output.stream_position()? <= MAX_ARTIFACT_BYTES,
            "Vorbis exceeds its size limit"
        );
        Ok(())
    };
    let mut remaining = audio.frames as usize;
    while remaining > 0 {
        check_cancel(cancel)?;
        let count = remaining.min(4096);
        let block = pcm_block(input, count * audio.channels as usize)?;
        emit(
            encoder
                .encode(&block)
                .map_err(|error| anyhow::anyhow!("Vorbis encoding failed: {error}"))?,
        )?;
        remaining -= count;
    }
    emit(
        encoder
            .flush()
            .map_err(|error| anyhow::anyhow!("Vorbis finalization failed: {error}"))?,
    )?;
    Ok(())
}

#[cfg(feature = "audio-recording")]
fn write_ogg_page(output: &mut impl Write, mut page: Vec<u8>, sequence: &mut u32) -> Result<()> {
    page[14..18].copy_from_slice(&0x5A45_4646u32.to_le_bytes());
    page[18..22].copy_from_slice(&sequence.to_le_bytes());
    set_ogg_crc(&mut page);
    output.write_all(&page)?;
    *sequence += 1;
    Ok(())
}

#[cfg(not(feature = "audio-recording"))]
fn ogg(
    _: &mut impl Read,
    _: AudioInfo,
    _: &[u8],
    _: &AtomicBool,
    _: &mut (impl Write + Seek),
) -> Result<()> {
    anyhow::bail!("Ogg Vorbis export requires the audio-recording build feature")
}

#[cfg(feature = "audio-recording")]
fn vorbis_comments(bytes: &[u8], comment: &[u8], cancel: &AtomicBool) -> Result<Vec<u8>> {
    let mut position = 0;
    let mut packet = Vec::new();
    let mut headers = Vec::new();
    while headers.len() < 3 {
        let page = ogg_page(bytes, position)?;
        let segments = &page[27..27 + page[26] as usize];
        let mut body = 27 + segments.len();
        for &length in segments {
            ensure!(
                headers.len() < 3,
                "Vorbis headers unexpectedly share a page with audio"
            );
            packet.extend_from_slice(&page[body..body + length as usize]);
            body += length as usize;
            if length < 255 {
                headers.push(std::mem::take(&mut packet));
            }
        }
        position += page.len();
    }
    for (index, signature) in [b"\x01vorbis", b"\x03vorbis", b"\x05vorbis"]
        .iter()
        .enumerate()
    {
        ensure!(
            headers[index].starts_with(*signature),
            "invalid encoded Vorbis header order"
        );
    }
    headers[1] = comment.to_vec();
    let mut output = Vec::new();
    let mut sequence = 0u32;
    for (index, header) in headers.iter().enumerate() {
        let mut laces = vec![255; header.len() / 255];
        laces.push((header.len() % 255) as u8);
        let mut used = 0;
        for (part, segments) in laces.chunks(255).enumerate() {
            check_cancel(cancel)?;
            let length = segments.iter().map(|value| *value as usize).sum::<usize>();
            let mut page = b"OggS\0\0".to_vec();
            page[5] = u8::from(part != 0) | if index == 0 && part == 0 { 2 } else { 0 };
            let granule = if *segments.last().expect("nonempty lacing table") < 255 {
                0u64
            } else {
                u64::MAX
            };
            page.extend_from_slice(&granule.to_le_bytes());
            page.extend_from_slice(&0x5A45_4646u32.to_le_bytes());
            page.extend_from_slice(&sequence.to_le_bytes());
            page.extend_from_slice(&[0; 4]);
            page.push(segments.len() as u8);
            page.extend_from_slice(segments);
            page.extend_from_slice(&header[used..used + length]);
            used += length;
            set_ogg_crc(&mut page);
            output.extend(page);
            sequence += 1;
        }
    }
    while position < bytes.len() {
        check_cancel(cancel)?;
        let mut page = ogg_page(bytes, position)?.to_vec();
        position += page.len();
        page[14..18].copy_from_slice(&0x5A45_4646u32.to_le_bytes());
        page[18..22].copy_from_slice(&sequence.to_le_bytes());
        set_ogg_crc(&mut page);
        output.extend(page);
        sequence += 1;
    }
    Ok(output)
}

#[cfg(feature = "audio-recording")]
fn ogg_page(bytes: &[u8], position: usize) -> Result<&[u8]> {
    let header = bytes
        .get(position..position + 27)
        .context("truncated encoded Ogg page")?;
    ensure!(&header[..5] == b"OggS\0", "invalid encoded Ogg page");
    let laces = bytes
        .get(position + 27..position + 27 + header[26] as usize)
        .context("truncated Ogg lacing table")?;
    let size = 27 + laces.len() + laces.iter().map(|value| *value as usize).sum::<usize>();
    bytes
        .get(position..position + size)
        .context("truncated encoded Ogg body")
}

#[cfg(feature = "audio-recording")]
fn set_ogg_crc(page: &mut [u8]) {
    page[22..26].fill(0);
    let mut crc = 0u32;
    for byte in page.iter() {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = (crc << 1)
                ^ if crc & 0x8000_0000 != 0 {
                    0x04C1_1DB7
                } else {
                    0
                };
        }
    }
    page[22..26].copy_from_slice(&crc.to_le_bytes());
}

fn chunk(output: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) -> anyhow::Result<()> {
    ensure!(
        payload.len() <= u32::MAX as usize
            && output.len() + payload.len() + 9 <= MAX_ARTIFACT_BYTES as usize,
        "WAV exceeds its size limit"
    );
    output.extend_from_slice(tag);
    output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    output.extend_from_slice(payload);
    if !payload.len().is_multiple_of(2) {
        output.push(0);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(super) fn wav(
    pcm: &[i16],
    channels: u16,
    sample_rate: u32,
    loop_range: Option<(u32, u32)>,
    pitch: Option<(u8, i16)>,
    metadata: &[u8],
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    encode(
        AudioFormat::Wav,
        AudioData {
            pcm,
            channels,
            sample_rate,
            loop_range,
            pitch,
        },
        metadata,
        cancel,
    )
}

fn wav_header(audio: AudioInfo, metadata: &[u8]) -> Result<Vec<u8>> {
    let AudioInfo {
        channels,
        sample_rate,
        loop_range,
        pitch,
        ..
    } = audio;
    let mut output = b"RIFF\0\0\0\0WAVE".to_vec();
    let mut format = Vec::new();
    format.extend_from_slice(&1u16.to_le_bytes());
    format.extend_from_slice(&channels.to_le_bytes());
    format.extend_from_slice(&sample_rate.to_le_bytes());
    format.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
    format.extend_from_slice(&(channels * 2).to_le_bytes());
    format.extend_from_slice(&16u16.to_le_bytes());
    chunk(&mut output, b"fmt ", &format)?;
    if pitch.is_some() || loop_range.is_some() {
        let (root, correction) = pitch.unwrap_or((60, 0));
        let mut sampler = vec![0u8; if loop_range.is_some() { 60 } else { 36 }];
        sampler[8..12].copy_from_slice(&(1_000_000_000u32 / sample_rate).to_le_bytes());
        let pitch = f64::from(root) - f64::from(correction) / 100.0;
        ensure!(
            (0.0..128.0).contains(&pitch),
            "WAV sample pitch is outside its MIDI note range"
        );
        let root = pitch.floor() as u32;
        let fraction =
            ((pitch - f64::from(root)) * 4_294_967_296.0).clamp(0.0, f64::from(u32::MAX)) as u32;
        sampler[12..16].copy_from_slice(&root.to_le_bytes());
        sampler[16..20].copy_from_slice(&fraction.to_le_bytes());
        if let Some((start, end)) = loop_range {
            ensure!(
                start < end && u64::from(end) <= audio.frames,
                "invalid WAV sample loop"
            );
            sampler[28..32].copy_from_slice(&1u32.to_le_bytes());
            sampler[44..48].copy_from_slice(&start.to_le_bytes());
            sampler[48..52].copy_from_slice(&(end - 1).to_le_bytes());
        }
        chunk(&mut output, b"smpl", &sampler)?;
    }
    let mut info = b"INFO".to_vec();
    let mut comment = metadata.to_vec();
    comment.push(0);
    chunk(&mut info, b"ICMT", &comment)?;
    chunk(&mut output, b"LIST", &info)?;
    ensure!(
        output.len() as u64 + 8 + audio.frames * u64::from(channels) * 2 <= MAX_ARTIFACT_BYTES,
        "WAV exceeds its size limit"
    );
    output.extend_from_slice(b"data");
    output.extend_from_slice(&((audio.frames * u64::from(channels) * 2) as u32).to_le_bytes());
    let length = output.len() as u64 - 8 + audio.frames * u64::from(channels) * 2;
    output[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    Ok(output)
}

#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{Metrics, PcmEvidence};

const MAX_REFERENCE_BYTES: u64 = 192_000 * 120 * 8;

#[derive(Debug, Serialize)]
pub(crate) struct ReferenceEvidence {
    byte_len: u64,
    f32_sha256: String,
    pcm: PcmEvidence,
    pub(crate) projected_pcm_matches: bool,
}

pub(crate) fn compare_f32_file(
    path: &Path,
    playback: &PcmEvidence,
    cancel: &AtomicBool,
) -> Result<ReferenceEvidence> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "reference validation cancelled"
    );
    let mut file = File::open(path).context("could not open native f32 reference")?;
    let metadata = file.metadata()?;
    let byte_len = metadata.len();
    ensure!(
        metadata.is_file()
            && byte_len > 0
            && byte_len.is_multiple_of(8)
            && byte_len <= MAX_REFERENCE_BYTES,
        "reference must be a nonempty stereo f32le file of at most {MAX_REFERENCE_BYTES} bytes"
    );
    let mut hash = Sha256::new();
    let mut metrics = Metrics::new(playback.sample_rate);
    let mut bytes = [0_u8; 8192];
    let mut samples = [0_i16; 2048];
    let mut remaining = byte_len;
    while remaining > 0 {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "reference validation cancelled"
        );
        let count = remaining.min(bytes.len() as u64) as usize;
        file.read_exact(&mut bytes[..count])?;
        hash.update(&bytes[..count]);
        for (index, raw) in bytes[..count].as_chunks::<4>().0.iter().enumerate() {
            let value = f32::from_le_bytes(*raw);
            ensure!(value.is_finite(), "reference contains a non-finite sample");
            samples[index] = (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        }
        metrics.push(&samples[..count / 4]);
        remaining -= count as u64;
    }
    ensure!(
        file.read(&mut bytes[..1])? == 0,
        "reference grew during validation"
    );
    let pcm = metrics.finish();
    Ok(ReferenceEvidence {
        byte_len,
        f32_sha256: const_hex::encode(hash.finalize()),
        projected_pcm_matches: pcm.frames == playback.frames
            && pcm.pcm_sha256 == playback.pcm_sha256,
        pcm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_preserves_native_projection_and_requires_the_whole_interval() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("native.f32");
        let values = [0.0_f32, -0.0, -1.0, 1.0, -2.0, 2.0, 0.000_01, -0.5];
        let raw: Vec<_> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        std::fs::write(&path, &raw)?;
        let mut metrics = Metrics::new(48_000);
        metrics.push(&[0, 0, -32767, 32767, -32767, 32767, 0, -16383]);
        let mut playback = metrics.finish();
        let cancel = AtomicBool::new(false);
        let result = compare_f32_file(&path, &playback, &cancel)?;
        assert!(result.projected_pcm_matches);
        assert_eq!(result.byte_len, raw.len() as u64);
        assert_eq!(result.f32_sha256, zeff_firmware::sha256_hex(&raw));
        playback.frames -= 1;
        assert!(!compare_f32_file(&path, &playback, &cancel)?.projected_pcm_matches);
        playback.frames += 1;
        playback.pcm_sha256 = "0".repeat(64);
        assert!(!compare_f32_file(&path, &playback, &cancel)?.projected_pcm_matches);
        Ok(())
    }

    #[test]
    fn invalid_reference_files_and_cancellation_are_rejected() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("native.f32");
        let playback = Metrics::new(48_000).finish();
        let cancel = AtomicBool::new(false);
        for bytes in [
            vec![],
            vec![0; 7],
            f32::NAN.to_le_bytes().repeat(2),
            f32::INFINITY.to_le_bytes().repeat(2),
        ] {
            std::fs::write(&path, bytes)?;
            assert!(compare_f32_file(&path, &playback, &cancel).is_err());
        }
        File::create(&path)?.set_len(MAX_REFERENCE_BYTES + 8)?;
        assert!(compare_f32_file(&path, &playback, &cancel).is_err());
        assert!(compare_f32_file(directory.path(), &playback, &cancel).is_err());
        assert!(compare_f32_file(&path, &playback, &AtomicBool::new(true)).is_err());
        Ok(())
    }
}

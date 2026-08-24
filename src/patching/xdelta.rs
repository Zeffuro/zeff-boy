use anyhow::{Context, ensure};

const VCDIFF_MAGIC: &[u8; 4] = b"\xD6\xC3\xC4\0";
const MAX_XDELTA_INPUT_BYTES: usize = 512 * 1024 * 1024;
const MAX_XDELTA_OUTPUT_BYTES: usize = 512 * 1024 * 1024;

pub(crate) fn apply_xdelta_patch(source: &[u8], patch: &[u8]) -> anyhow::Result<Vec<u8>> {
    ensure!(patch.len() >= 5, "xdelta patch header truncated");
    ensure!(
        &patch[..4] == VCDIFF_MAGIC,
        "xdelta patch missing VCDIFF header"
    );
    ensure!(
        source.len().saturating_add(patch.len()) <= MAX_XDELTA_INPUT_BYTES,
        "xdelta source and patch exceed the 512 MiB in-memory limit"
    );

    let secondary = if patch[4] & 0x01 != 0 {
        ensure!(
            patch.len() >= 6,
            "xdelta secondary-compressor header truncated"
        );
        Some(patch[5])
    } else {
        None
    };

    let output = match secondary {
        Some(1 | 16) => xdelta3::decode(patch, source)
            .context("xdelta3 DJW/FGK decode failed (wrong source file or corrupt patch)")?,
        None | Some(2) => oxidelta::engine::decode(source, patch)
            .map_err(anyhow::Error::msg)
            .context("xdelta3/VCDIFF decode failed (wrong source file or corrupt patch)")?,
        Some(id) => anyhow::bail!("unsupported xdelta secondary compressor {id}"),
    };

    ensure!(
        output.len() <= MAX_XDELTA_OUTPUT_BYTES,
        "xdelta output exceeds the 512 MiB in-memory limit"
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use oxidelta::compress::encoder::{self, CompressOptions};
    use oxidelta::compress::secondary::SecondaryCompression;

    use super::*;

    #[test]
    fn applies_xdelta3_djw_patch() {
        let source = b"the original PC Engine track";
        let target = b"the translated PC Engine track";
        let patch = xdelta3::encode(target, source).unwrap();

        assert_eq!(apply_xdelta_patch(source, &patch).unwrap(), target);
    }

    #[test]
    fn applies_xdelta3_lzma_patch() {
        let source = b"the original PC Engine track";
        let target = b"the translated PC Engine track";
        let mut patch = Vec::new();
        let options = CompressOptions {
            secondary: SecondaryCompression::Lzma,
            ..CompressOptions::default()
        };
        encoder::encode_all(&mut patch, source, target, options).unwrap();

        assert_eq!(apply_xdelta_patch(source, &patch).unwrap(), target);
    }
}

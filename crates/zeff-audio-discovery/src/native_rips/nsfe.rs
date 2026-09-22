use std::sync::atomic::AtomicBool;

use anyhow::Result;

use super::{NativeRip, NativeRipFormat};
use crate::nes_native::NesNativeSong;

pub(super) fn encode(bytes: &[u8], song: &NesNativeSong, cancel: &AtomicBool) -> Result<NativeRip> {
    let mut rip = super::nes::encode(bytes, song, cancel)?;
    let nsf = &rip.bytes;
    let mut output = b"NSFE".to_vec();
    let mut info = nsf[8..14].to_vec();
    info.extend([nsf[0x7a], nsf[0x7b], 1, 0]);
    chunk(&mut output, b"INFO", &info)?;
    let mut rate = nsf[0x6e..0x70].to_vec();
    rate.extend_from_slice(&nsf[0x78..0x7a]);
    chunk(&mut output, b"RATE", &rate)?;
    chunk(&mut output, b"DATA", &nsf[0x80..])?;
    let mut label = song.title.replace('\0', " ").into_bytes();
    label.push(0);
    chunk(&mut output, b"tlbl", &label)?;
    chunk(&mut output, b"NEND", &[])?;
    rip.bytes = output;
    rip.metadata.format = NativeRipFormat::Nsfe;
    rip.metadata.warnings.push(
        "Requires an NSFe player supporting the RATE chunk. Imported NSFe playback and NSF2 execution are not qualified.".into(),
    );
    Ok(rip)
}

fn chunk(output: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) -> Result<()> {
    output.extend(u32::try_from(data.len())?.to_le_bytes());
    output.extend(id);
    output.extend(data);
    Ok(())
}

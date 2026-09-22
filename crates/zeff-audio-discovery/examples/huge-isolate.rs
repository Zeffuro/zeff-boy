use std::io::{Read, Write};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};
use zeff_audio_discovery::huge::{discovery, isolation};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 4,
        "usage: huge-isolate ROM DESCRIPTOR_OFFSET FILL_BYTE OUTPUT_ROM"
    );
    let number = |index: usize| -> Result<u16> {
        let text = args[index]
            .to_str()
            .context("numeric argument must be text")?;
        Ok(match text.strip_prefix("0x") {
            Some(hex) => u16::from_str_radix(hex, 16)?,
            None => text.parse()?,
        })
    };
    let descriptor = number(1)?;
    let fill = u8::try_from(number(2)?)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(0x8001)
        .read_to_end(&mut bytes)?;
    let cancel = AtomicBool::new(false);
    let report = discovery::discover(&bytes, Default::default(), &cancel)
        .map_err(|stop| anyhow::anyhow!("discovery stopped: {stop:?}"))?;
    let selected: Vec<_> = report
        .bound
        .iter()
        .filter(|b| b.song.descriptor.offset == u32::from(descriptor))
        .collect();
    ensure!(
        selected.len() == 1,
        "descriptor must bind exactly one supported driver"
    );
    let isolated = isolation::build(&bytes, selected[0], fill, &cancel)?;
    let report = serde_json::json!({
        "schema":"zeff-huge-isolation/1",
        "source_sha256":zeff_firmware::sha256_hex(&bytes),
        "isolated_sha256":zeff_firmware::sha256_hex(&isolated.bytes),
        "fill":fill,
        "isolation":isolated,
        "limitation":"Experimental DMG VBlank player; requires per-source execution/state proof before catalog or native export admission."
    });
    let serialized = serde_json::to_string_pretty(&report)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?
        .write_all(&isolated.bytes)?;
    println!("{serialized}");
    Ok(())
}

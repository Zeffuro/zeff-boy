use std::io::Read;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        (1..=2).contains(&args.len()),
        "usage: huge-inspect ROM [DESCRIPTOR_OFFSET]"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(0x8001)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() == 0x8000,
        "inspection requires an exact 32 KiB ROM"
    );
    if args.len() == 1 {
        let discovery = zeff_audio_discovery::huge::discovery::discover(
            &bytes,
            Default::default(),
            &AtomicBool::new(false),
        )
        .map_err(|stop| anyhow::anyhow!("discovery stopped: {stop:?}"))?;
        let report = serde_json::json!({
            "schema": "zeff-huge-discovery/1",
            "source_sha256": zeff_firmware::sha256_hex(&bytes),
            "format_reference_revision": zeff_audio_discovery::huge::SOURCE_REVISION,
            "binding": "static_literal_init",
            "discovery": discovery,
            "root_bits": {"reset":1,"vblank":2,"lcd":4,"timer":8,"serial":16,"joypad":32},
            "limitation": "Static paths do not prove execution, interrupt enablement, update cadence, complete song enumeration, or native export safety."
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let address = args[1].to_str().context("descriptor offset must be text")?;
    let descriptor = match address.strip_prefix("0x") {
        Some(hex) => u16::from_str_radix(hex, 16)?,
        None => address.parse::<u16>()?,
    };
    let song = zeff_audio_discovery::huge::inspect_plain_song(
        &bytes,
        descriptor,
        Default::default(),
        &AtomicBool::new(false),
    )
    .map_err(|stop| anyhow::anyhow!("inspection stopped: {stop:?}"))?
    .context("descriptor is outside the supported effect-free ROM-only format")?;
    let report = serde_json::json!({
        "schema": "zeff-huge-structure/1",
        "source_sha256": zeff_firmware::sha256_hex(&bytes),
        "format_reference_revision": zeff_audio_discovery::huge::SOURCE_REVISION,
        "binding": "explicit_descriptor",
        "song": song,
        "limitation": "Structural inspection does not identify an active driver or qualify native playback/export."
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

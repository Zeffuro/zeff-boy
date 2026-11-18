#[cfg(not(target_arch = "wasm32"))]
fn run() -> anyhow::Result<()> {
    use anyhow::{bail, ensure};
    use sha2::{Digest, Sha256};
    use zeff_md_core::{ControllerKind, Region, ram_census};
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 5,
        "usage: ram_copy_census RAW_ROM RAW_STATE_OR_- us|jp|pal 3|6 FIELDS"
    );
    let rom = std::fs::read(&args[0])?;
    let restore = if args[1] == "-" {
        None
    } else {
        Some(std::fs::read(&args[1])?)
    };
    let region = match args[2].as_str() {
        "us" => Region::NtscUs,
        "jp" => Region::NtscJapan,
        "pal" => Region::Pal,
        _ => bail!("unknown region"),
    };
    let controller = match args[3].as_str() {
        "3" => ControllerKind::ThreeButton,
        "6" => ControllerKind::SixButton,
        _ => bail!("unknown controller"),
    };
    let hex = |bytes: &[u8]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    println!(
        "ram-copy-census-v1 executable_sha256={} rom_sha256={} restore_sha256={}",
        hex(&Sha256::digest(std::fs::read(std::env::current_exe()?)?)),
        hex(&Sha256::digest(&rom)),
        restore
            .as_ref()
            .map_or_else(|| "-".into(), |bytes| hex(&Sha256::digest(bytes)))
    );
    let result = ram_census::run(
        rom,
        restore.as_deref(),
        region,
        controller,
        args[4].parse()?,
    )?;
    println!("{result:?}");
    for (name, value) in ram_census::METRIC_NAMES.iter().zip(result.counters) {
        println!("metric {name}={value}");
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let result = std::thread::Builder::new()
        .name("ram-copy-census".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .and_then(|worker| {
            worker
                .join()
                .map_err(|_| std::io::Error::other("census panicked"))
        });
    match result {
        Ok(Ok(())) => {}
        result => {
            eprintln!("census failed: {result:?}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}

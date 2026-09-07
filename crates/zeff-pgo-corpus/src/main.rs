use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let flag = args
        .next()
        .ok_or("usage: zeff-pgo-corpus --output-dir <fresh-dir>")?;
    if flag != std::ffi::OsStr::new("--output-dir") {
        return Err("usage: zeff-pgo-corpus --output-dir <fresh-dir>".into());
    }
    let output_dir = PathBuf::from(
        args.next()
            .ok_or("--output-dir requires a fresh directory")?,
    );
    if args.next().is_some() {
        return Err("usage: zeff-pgo-corpus --output-dir <fresh-dir>".into());
    }
    let manifest = zeff_pgo_corpus::write_fresh_corpus(&output_dir)?;
    println!(
        "wrote {} fixtures to {}",
        manifest.fixtures.len(),
        output_dir.display()
    );
    Ok(())
}

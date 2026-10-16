use std::path::PathBuf;

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let first = args
        .next()
        .ok_or("usage: verify-compare-remainder [--source-only] <export-jsonl>")?;
    let source_only = first == "--source-only";
    let path = PathBuf::from(if source_only {
        args.next().ok_or("missing comparison export")?
    } else {
        first
    });
    if args.next().is_some() {
        return Err("unexpected comparison argument".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let count = if source_only {
        zeff_m68k_verify::compare_remainder::qualify_export(&text)?
    } else {
        zeff_m68k_verify::compare_remainder::verify_export(&text)?
    };
    println!("comparison remainder count={count} bad=0 source_only={source_only}");
    Ok(())
}

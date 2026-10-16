use std::path::PathBuf;

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or("usage: verify-tst <original-export-jsonl>")?,
    );
    if args.next().is_some() {
        return Err("usage: verify-tst <original-export-jsonl>".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let count = zeff_m68k_verify::tst::verify_export(&text)?;
    println!("physical TST original count={count} bad=0");
    Ok(())
}

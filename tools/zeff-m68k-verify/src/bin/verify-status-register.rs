use std::path::PathBuf;
use zeff_m68k_verify::status_register::{validate_original_payload, verify_payload};

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or("usage: verify-status-register <original-jsonl>")?,
    );
    if args.next().is_some() {
        return Err("usage: verify-status-register <original-jsonl>".into());
    }
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    validate_original_payload(&text)?;
    let count = verify_payload(&text)?;
    println!("physical status register original count={count} bad=0");
    Ok(())
}

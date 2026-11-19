#[path = "../reference.rs"]
mod reference;
#[path = "../clear_not/remainder.rs"]
mod remainder;

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: verify-clear-not-remainder <unary-export> <remaining-export>".into());
    }
    let first = std::fs::read_to_string(&args[0]).map_err(|e| e.to_string())?;
    let second = std::fs::read_to_string(&args[1]).map_err(|e| e.to_string())?;
    let count = remainder::verify_exports([&first, &second])?;
    println!("CLR/NOT remainder original count={count} bad=0");
    Ok(())
}

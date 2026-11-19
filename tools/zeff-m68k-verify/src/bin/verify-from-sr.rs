use std::{env, fs};
use zeff_m68k_verify::from_sr::{Selection, validate_original_payload, verify_payload};

fn main() -> Result<(), String> {
    let paths: Vec<_> = env::args().skip(1).collect();
    if paths.len() != 2 {
        return Err("usage: verify-from-sr <firstodd.jsonl> <completion.jsonl>".into());
    }
    for ((selection, expected), path) in [(Selection::FirstOdd, 485), (Selection::Completion, 781)]
        .into_iter()
        .zip(paths)
    {
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        validate_original_payload(&text, selection)?;
        let count = verify_payload(&text, selection)?;
        if count != expected {
            return Err(format!("{selection:?}: expected {expected}, found {count}"));
        }
        println!("{selection:?}: {count} passed, 0 failures, 0 capture gaps");
    }
    Ok(())
}

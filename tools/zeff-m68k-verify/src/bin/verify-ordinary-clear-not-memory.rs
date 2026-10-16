use std::{env, fs};
use zeff_m68k_verify::clear_not::{
    Selection, parse_jsonl, validate_original_payload, verify_ordinary_completion,
};

fn main() -> Result<(), String> {
    let paths: Vec<_> = env::args().skip(1).collect();
    if paths.len() != 1 {
        return Err("usage: verify-ordinary-clear-not-memory <completion.jsonl>".into());
    }
    let text = fs::read_to_string(&paths[0]).map_err(|error| error.to_string())?;
    validate_original_payload(&text, Selection::Completion)?;
    let cases = parse_jsonl(&text)?;
    if cases.len() != 2199 {
        return Err(format!("expected 2199 completions, found {}", cases.len()));
    }
    for case in &cases {
        verify_ordinary_completion(case)
            .map_err(|error| format!("{}/{}: {error}", case.cohort, case.name))?;
    }
    println!("Ordinary completion: 2199 passed, 0 failures");
    Ok(())
}

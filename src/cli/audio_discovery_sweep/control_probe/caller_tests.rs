use super::tests::{capture, fixture};
use super::*;
use anyhow::Result;

fn source_with_caller(caller: &[u8]) -> Vec<u8> {
    let mut source = fixture();
    source[16 + 0x80..16 + 0x80 + caller.len()].copy_from_slice(caller);
    source[16 + 0x200..16 + 0x209]
        .copy_from_slice(&[0xea, 0xb9, 0, 0x81, 0xea, 0x8d, 2, 0x40, 0x60]);
    source[16 + 0x101] = 0x80;
    source[16 + 0x104] = 0x80;
    source
}

fn inspect(source: &[u8], button: bool) -> Result<Value> {
    let (row, inventory, trace) = capture(source, button)?;
    let result = observe(source, &trace, &row, &inventory, &AtomicBool::new(false));
    assert_eq!(result["status"], "complete", "{result}");
    Ok(result)
}

#[test]
fn native_ram_shifts_bind_the_consumed_child_argument() -> Result<()> {
    let source = source_with_caller(&[
        0x85, 0xad, 0xa5, 0xad, 0x0a, 0x0a, 0xa8, 0x20, 0, 0x82, 0x60,
    ]);
    let mut hashes = Vec::new();
    for button in [false, true] {
        let result = inspect(&source, button)?;
        let links = result["caller_argument_reads"].as_array().unwrap();
        assert_eq!(links.len(), 1, "{result}");
        let link = &links[0];
        let index = link["entry_argument_read_index"].as_u64().unwrap() as usize;
        let argument_link = &result["entry_argument_reads"][index];
        assert_eq!(link["event_index"], argument_link["event_index"]);
        assert_eq!(link["callee_entry"], argument_link["entry"]);
        assert_eq!(link["argument"], argument_link["argument"]);
        assert_eq!(link["call"], argument_link["call"]);
        assert_eq!(link["caller_entry"]["pc"], 0x8080);
        assert_eq!(link["callee_entry"]["pc"], 0x8200);
        assert_eq!(
            link["argument"],
            json!({"register": "y", "value": u8::from(button) * 4})
        );
        assert_eq!(link["ram_read"]["pc"], 0x8082);
        assert_eq!(link["ram_read"]["address"], 0xad);
        assert_eq!(link["ram_read"]["canonical_address"], 0xad);
        assert_eq!(link["ram_read"]["value"], u8::from(button));
        let transforms = link["transforms"].as_array().unwrap();
        assert_eq!(transforms.len(), 2);
        assert_eq!(transforms[0]["input"], u8::from(button));
        assert_eq!(transforms[0]["output"], u8::from(button) * 2);
        assert_eq!(transforms[1]["input"], transforms[0]["output"]);
        assert_eq!(transforms[1]["output"], u8::from(button) * 4);
        let witnesses = link["witnesses"].as_array().unwrap();
        assert_eq!(witnesses.len(), 6);
        assert_eq!(witnesses.last().unwrap()["bytes"], "200082");
        assert!(
            link["ram_read"]["cpu_cycle"].as_u64().unwrap()
                < link["callee_entry"]["cpu_cycle"].as_u64().unwrap()
        );
        assert_eq!(
            argument_link["rom_read"]["address"],
            0x8100 + u16::from(button) * 4
        );
        assert_eq!(
            argument_link["rom_read"]["value"],
            if button { 0x80 } else { 0x40 }
        );
        hashes.push(result["verification"]["native_f32_sha256"].clone());
    }
    assert_ne!(hashes[0], hashes[1]);
    Ok(())
}

#[test]
fn native_ram_alias_and_wrapping_shifts_remain_explicit() -> Result<()> {
    let source = source_with_caller(&[
        0x09, 0x80, 0x85, 0xad, 0xad, 0xad, 0x08, 0x0a, 0x0a, 0xa8, 0x20, 0, 0x82, 0x60,
    ]);
    let result = inspect(&source, false)?;
    let link = &result["caller_argument_reads"][0];
    assert_eq!(result["caller_argument_reads"].as_array().unwrap().len(), 1);
    assert_eq!(link["ram_read"]["address"], 0x08ad);
    assert_eq!(link["ram_read"]["canonical_address"], 0xad);
    assert_eq!(link["ram_read"]["value"], 128);
    assert_eq!(link["transforms"][0]["output"], 0);
    assert_eq!(link["transforms"][0]["carry"], true);
    assert_eq!(link["transforms"][1]["output"], 0);
    assert_eq!(link["transforms"][1]["carry"], false);
    assert_eq!(link["argument"]["value"], 0);
    Ok(())
}

#[test]
fn native_clobbers_and_non_ram_reads_do_not_bind_equal_arguments() -> Result<()> {
    for caller in [
        vec![
            0x85, 0xad, 0xa5, 0xad, 0x0a, 0x0a, 0xa8, 0xa0, 4, 0x20, 0, 0x82, 0x60,
        ],
        vec![
            0x85, 0xad, 0xa5, 0xad, 0x0a, 0x0a, 0x49, 0, 0xa8, 0x20, 0, 0x82, 0x60,
        ],
        vec![0xad, 0, 0x81, 0x0a, 0x0a, 0xa8, 0x20, 0, 0x82, 0x60],
        vec![0xad, 0x16, 0x40, 0x0a, 0x0a, 0xa8, 0x20, 0, 0x82, 0x60],
    ] {
        let result = inspect(&source_with_caller(&caller), true)?;
        assert_eq!(result["entry_argument_reads"].as_array().unwrap().len(), 1);
        assert_eq!(result["caller_argument_reads"], json!([]), "{result}");
    }
    Ok(())
}

#[test]
fn native_caller_budget_includes_the_child_call() -> Result<()> {
    for (nops, count) in [(61, 1), (62, 0)] {
        let mut caller = vec![0x85, 0xad];
        caller.extend(std::iter::repeat_n(0xea, nops));
        caller.extend_from_slice(&[0xa5, 0xad, 0x20, 0, 0x82, 0x60]);
        let mut source = source_with_caller(&caller);
        source[16 + 0x200..16 + 0x208].copy_from_slice(&[0xa8, 0xb9, 0, 0x81, 0x8d, 2, 0x40, 0x60]);
        let result = inspect(&source, true)?;
        assert_eq!(result["entry_argument_reads"].as_array().unwrap().len(), 1);
        assert_eq!(
            result["caller_argument_reads"].as_array().unwrap().len(),
            count
        );
        if count == 1 {
            assert_eq!(
                result["caller_argument_reads"][0]["witnesses"]
                    .as_array()
                    .unwrap()
                    .len(),
                64
            );
        }
    }
    Ok(())
}

#[test]
fn native_child_sink_budget_keeps_the_last_proven_link() -> Result<()> {
    for (nops, count) in [(62, 1), (63, 0)] {
        let mut source = source_with_caller(&[0x85, 0xad, 0xa5, 0xad, 0xa8, 0x20, 0, 0x82, 0x60]);
        let mut child = vec![0xea; nops];
        child.extend_from_slice(&[0xb9, 0, 0x81, 0x8d, 2, 0x40, 0x60]);
        source[16 + 0x200..16 + 0x200 + child.len()].copy_from_slice(&child);
        let result = inspect(&source, true)?;
        assert_eq!(
            result["entry_argument_reads"].as_array().unwrap().len(),
            count
        );
        assert_eq!(
            result["caller_argument_reads"].as_array().unwrap().len(),
            count
        );
        if count == 1 {
            assert_eq!(
                result["entry_argument_reads"][0]["witnesses"]
                    .as_array()
                    .unwrap()
                    .len(),
                64
            );
        }
    }
    Ok(())
}

#[test]
fn native_caller_links_are_withheld_after_failed_verification() -> Result<()> {
    let source = source_with_caller(&[0x85, 0xad, 0xa5, 0xad, 0xa8, 0x20, 0, 0x82, 0x60]);
    let (mut row, inventory, trace) = capture(&source, true)?;
    row["validation"]["native_reference"]["evidence"]["f32_sha256"] = json!("f".repeat(64));
    let result = observe(&source, &trace, &row, &inventory, &AtomicBool::new(false));
    assert_eq!(result["reason"], "replay_pcm_mismatch");
    assert_eq!(result["caller_argument_reads"], json!([]));
    let cancelled = observe(&source, &trace, &row, &inventory, &AtomicBool::new(true));
    assert_eq!(cancelled["reason"], "cancelled");
    assert_eq!(cancelled["caller_argument_reads"], json!([]));
    Ok(())
}

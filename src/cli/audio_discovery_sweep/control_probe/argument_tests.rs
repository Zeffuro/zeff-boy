use super::tests::{capture, fixture};
use super::*;
use anyhow::Result;

fn source_with_driver(driver: &[u8]) -> Vec<u8> {
    let mut source = fixture();
    source[16 + 0x80..16 + 0x80 + driver.len()].copy_from_slice(driver);
    source
}

fn inspect(source: &[u8], button: bool) -> Result<Value> {
    let (row, inventory, trace) = capture(source, button)?;
    let result = observe(source, &trace, &row, &inventory, &AtomicBool::new(false));
    assert_eq!(result["status"], "complete", "{result}");
    Ok(result)
}

#[test]
fn native_argument_copies_and_branch_reach_the_sound_sink() -> Result<()> {
    let source = source_with_driver(&[
        0xea, 0xaa, 0xe0, 2, 0x90, 1, 0xea, 0xbd, 0, 0x81, 0xa8, 0xa9, 0, 0xea, 0x8c, 2, 0x40, 0x60,
    ]);
    let mut hashes = Vec::new();
    for button in [false, true] {
        let result = inspect(&source, button)?;
        assert_eq!(result["entry_index_reads"], json!([]));
        let links = result["entry_argument_reads"].as_array().unwrap();
        assert_eq!(links.len(), 1, "{links:?}");
        let link = &links[0];
        assert_eq!(
            link["argument"],
            json!({"register": "a", "value": u8::from(button)})
        );
        assert_eq!(link["entry"]["pc"], 0x8080);
        assert_eq!(link["rom_read"]["index_register"], "x");
        assert_eq!(link["rom_read"]["index_value"], u8::from(button));
        assert_eq!(
            link["rom_read"]["source_offset"],
            16 + 0x100 + usize::from(button)
        );
        assert_eq!(link["rom_read"]["value"], if button { 0x80 } else { 0x40 });
        assert_eq!(link["writer_pc"], 0x808e);
        assert_eq!(link["register"], 0x4002);
        let observation = result["observations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|observation| observation["event_index"] == link["event_index"])
            .unwrap();
        assert_eq!(observation["pc"], link["writer_pc"]);
        assert_eq!(observation["register"], link["register"]);
        assert_eq!(observation["value"], link["rom_read"]["value"]);
        assert_eq!(observation["cycle"], link["write_trace_cycle"]);
        assert_eq!(
            observation["call_path"][0]["entry"]["cycle"],
            link["entry"]["cpu_cycle"]
        );
        assert!(
            link["entry"]["cpu_cycle"].as_u64().unwrap()
                < link["rom_read"]["cpu_cycle"].as_u64().unwrap()
        );
        assert!(
            link["rom_read"]["cpu_cycle"].as_u64().unwrap()
                < link["write_trace_cycle"].as_u64().unwrap() + 7
        );
        let pcs: Vec<_> = link["witnesses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|witness| witness["pc"].as_u64().unwrap())
            .collect();
        assert_eq!(
            pcs,
            [
                0x8080, 0x8081, 0x8082, 0x8084, 0x8087, 0x808a, 0x808b, 0x808d, 0x808e
            ]
        );
        hashes.push(result["verification"]["native_f32_sha256"].clone());
    }
    assert_ne!(hashes[0], hashes[1]);
    Ok(())
}

#[test]
fn native_equal_value_overwrites_do_not_preserve_argument_provenance() -> Result<()> {
    for driver in [
        vec![0xa2, 1, 0xbd, 0, 0x81, 0xea, 0x8d, 2, 0x40, 0x60],
        vec![0xbd, 0, 0x81, 0xa8, 0xa0, 0x80, 0x8c, 2, 0x40, 0x60],
        vec![0xbd, 0, 0x81, 0x49, 0, 0x8d, 2, 0x40, 0x60],
    ] {
        let result = inspect(&source_with_driver(&driver), true)?;
        assert_eq!(result["entry_argument_reads"], json!([]), "{result}");
    }
    Ok(())
}

#[test]
fn native_arithmetic_kills_only_its_destination_lineage() -> Result<()> {
    let source = source_with_driver(&[
        0xa8, 0xa9, 1, 0x05, 0x10, 0xa2, 0, 0xb9, 0, 0x81, 0xea, 0x9d, 2, 0x40, 0x60,
    ]);
    let result = inspect(&source, true)?;
    let links = result["entry_argument_reads"].as_array().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["argument"], json!({"register": "a", "value": 1}));
    assert_eq!(links[0]["rom_read"]["index_register"], "y");
    assert_eq!(links[0]["rom_read"]["value"], 0x80);
    Ok(())
}

#[test]
fn native_expired_argument_window_keeps_capture_without_links() -> Result<()> {
    let mut driver = vec![0xea; 64];
    driver.extend_from_slice(&[0xbd, 0, 0x81, 0x8d, 2, 0x40, 0x60]);
    let result = inspect(&source_with_driver(&driver), true)?;
    assert_eq!(result["entry_argument_reads"], json!([]));
    assert_eq!(result["observed_writes"], 6);
    assert_eq!(result["argument_flow_contract"]["max_instructions"], 64);
    Ok(())
}

#[test]
fn failed_native_validation_discards_all_argument_links() -> Result<()> {
    let source = fixture();
    let (mut row, inventory, trace) = capture(&source, true)?;
    row["validation"]["native_reference"]["evidence"]["f32_sha256"] = json!("f".repeat(64));
    let result = observe(&source, &trace, &row, &inventory, &AtomicBool::new(false));
    assert_eq!(result["reason"], "replay_pcm_mismatch");
    assert_eq!(result["entry_argument_reads"], json!([]));
    let cancelled = observe(&source, &trace, &row, &inventory, &AtomicBool::new(true));
    assert_eq!(cancelled["reason"], "cancelled");
    assert_eq!(cancelled["entry_argument_reads"], json!([]));
    Ok(())
}

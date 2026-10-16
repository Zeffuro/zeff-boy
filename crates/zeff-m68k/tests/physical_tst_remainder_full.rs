#[path = "physical_tst_remainder_first.rs"]
mod first;
#[path = "physical_tst_remainder_full/mod.rs"]
pub mod full;
pub fn section(index: u32) -> (u32, u32) {
    match index {
        0 => {
            let (count, bad, unsupported) = first::run_first();
            (count, bad + unsupported)
        }
        1..=31 => full::section_with(index, |_| {}),
        32 => panic!("raw1698 is qualified by the external tools validator and WASM adapter"),
        _ => panic!("TST section"),
    }
}
#[test]
fn full_synthetic_sections() {
    for index in 0..32 {
        let result = section(index);
        eprintln!(
            "physical TST full section={index} count={} bad={}",
            result.0, result.1
        );
        assert_eq!(result, (full::COUNTS[index as usize], 0));
    }
}

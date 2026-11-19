#[path = "physical_compare_remainder_full/mod.rs"]
#[allow(dead_code)]
mod full;

#[test]
fn independent38_full_numeric_literal_trees() {
    let result = full::mini_section();
    eprintln!("compare-full-mini {} {}", result.0, result.1);
    assert_eq!(result, (38, 0));
}
#[test]
fn all37_finite_products() {
    for (index, count) in full::COUNTS.into_iter().enumerate() {
        let result = full::section(index as u32);
        eprintln!("compare-full {index} {} {}", result.0, result.1);
        assert_eq!(result, (count, 0), "section {index}");
    }
}

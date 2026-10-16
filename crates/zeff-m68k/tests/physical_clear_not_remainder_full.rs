#[path = "physical_clear_not_remainder_full/mod.rs"]
#[allow(dead_code)]
mod full;

#[test]
fn retained_numeric_literals() {
    assert_eq!(full::mini_section(), (459, 0));
    assert_eq!(full::directed_mini_section(), (246, 0));
    assert_eq!(full::construction_mini_section(), (40, 0));
}

#[test]
fn source_numeric_literals() {
    assert_eq!(full::status_mini_section(), (53, 0));
    assert_eq!(full::factor_source_section(), (129, 0));
}

#[test]
fn all_finite_products() {
    let mut total = 0;
    for (index, expected) in full::COUNTS.into_iter().enumerate() {
        let (count, bad) = full::section_with(index as u32, |_| {});
        eprintln!("clear-not full section={index} count={count} bad={bad}");
        assert_eq!((count, bad), (expected, 0));
        total += count;
    }
    assert_eq!(total, 8652952);
}

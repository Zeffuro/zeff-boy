use super::*;

#[test]
fn golden_word_predecrement_and_long_postincrement_have_distinct_old_source_states() {
    for supervisor in [false, true] {
        let control = if supervisor { SUPERVISOR } else { 0 };
        for size in [Size::Word, Size::Long] {
            let mut case = Case::new(Form {
                size,
                ea: if size == Size::Word {
                    Ea::Pre
                } else {
                    Ea::Post
                },
            });
            case.r = registers(supervisor, 31);
            case.r.pc = 0x1000;
            case.r.usp = 0x8000;
            case.r.ssp = 0x8000;
            case.register = 7;
            case.value = if size == Size::Word {
                0x8001
            } else {
                0x8000_0001
            };
            let (_, bus) = case.seed();
            let plan = oracle::plan(case, &bus.words);
            let tuples: Vec<_> = plan
                .iter()
                .map(|t| {
                    (
                        t.access.logical_address,
                        t.internal_pc,
                        t.ird,
                        t.registers.status_register(),
                        t.registers.address_register(7).unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                tuples,
                if size == Size::Word {
                    vec![
                        (0x7ffe, 0x1004, 0x4a67, control | 31, 0x7ffe),
                        (0x1004, 0x1004, 0x7201, control | 24, 0x7ffe),
                    ]
                } else {
                    vec![
                        (0x8000, 0x1002, 0x4a9f, control | 31, 0x8000),
                        (0x8002, 0x1002, 0x4a9f, control | 31, 0x8004),
                        (0x1004, 0x1004, 0x7201, control | 24, 0x8004),
                    ]
                }
            );
            oracle::completed(case, false);
            oracle::completed(case, true);
            for at in 0..case.form.count() {
                for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    capture::failed(case, at, failure);
                }
            }
        }
    }
}

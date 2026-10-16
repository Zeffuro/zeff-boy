use super::*;

#[test]
fn processing_classifier_recognizes_all_1952_legal_address_arithmetic_words() {
    let mut by_size = [0; 2];
    for family in [0xd000, 0x9000] {
        for (index, size_bits) in [0xc0, 0x1c0].into_iter().enumerate() {
            for destination in 0..8 {
                for mode in 0..8 {
                    for register in 0..if mode == 7 { 5 } else { 8 } {
                        let mut case = Case::new(Form {
                            operation: Operation::Add,
                            size: Size::Long,
                            ea: Ea::Post,
                        });
                        case.next = family | destination << 9 | size_bits | mode << 3 | register;
                        capture::failed(case, 2, Failure::Boundary);
                        by_size[index] += 1;
                    }
                }
            }
        }
    }
    assert_eq!(by_size, [976, 976]);
    assert_eq!(by_size.into_iter().sum::<usize>(), 1952);
}

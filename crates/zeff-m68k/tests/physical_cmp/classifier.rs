use super::*;

#[test]
fn long_post_final_capture_preserves_o_for_all_1400_cmp_next_words() {
    let mut by_size = [0; 3];
    for (size, size_bits) in [(Size::Byte, 0), (Size::Word, 0x40), (Size::Long, 0x80)] {
        let index = match size {
            Size::Byte => 0,
            Size::Word => 1,
            Size::Long => 2,
        };
        for destination in 0..8 {
            for mode in 0..8 {
                if size == Size::Byte && mode == 1 {
                    continue;
                }
                for register in 0..if mode == 7 { 5 } else { 8 } {
                    let mut case = Case::new(Form {
                        size: Size::Long,
                        ea: Ea::Post,
                    });
                    case.next = 0xb000 | destination << 9 | size_bits | mode << 3 | register;
                    capture::failed(case, 2, Failure::Boundary);
                    by_size[index] += 1;
                }
            }
        }
    }
    assert_eq!(by_size, [424, 488, 488]);
    assert_eq!(by_size.into_iter().sum::<usize>(), 1400);
}

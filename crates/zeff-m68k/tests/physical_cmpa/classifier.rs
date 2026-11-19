use super::*;

#[test]
fn long_post_final_capture_preserves_o_for_all_976_cmpa_next_words() {
    let mut by_size = [0; 2];
    for (index, size_bits) in [0xc0, 0x1c0].into_iter().enumerate() {
        for destination in 0..8 {
            for mode in 0..8 {
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
    assert_eq!(by_size, [488, 488]);
    assert_eq!(by_size.into_iter().sum::<usize>(), 976);
}

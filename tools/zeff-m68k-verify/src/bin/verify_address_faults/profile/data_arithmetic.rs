use super::*;

pub(super) fn legal(word: u16) -> bool {
    let (family, size, mode) = (word >> 12, word >> 6 & 7, word >> 3 & 7);
    matches!(family, 8 | 9 | 12 | 13)
        && size <= 2
        && !(mode == 7 && word & 7 > 4)
        && !(mode == 1 && (size == 0 || matches!(family, 8 | 12)))
}

pub(super) fn cohort_matches(case: &Case) -> bool {
    let op = case.initial.prefetch[0];
    let family = case.cohort.split_once('.').unwrap().0;
    let top = op >> 12;
    let opmode = op >> 6 & 7;
    let size = match case.cohort.rsplit_once('.').unwrap().1 {
        "b" => 0,
        "w" => 1,
        "l" => 2,
        _ => return false,
    };
    if matches!(opmode, 3 | 7) && matches!(top, 9 | 13) {
        return matches!((family, top), ("ADD", 13) | ("SUB", 9))
            && size == if opmode == 3 { 1 } else { 2 };
    }
    if size != opmode & 3 || size == 3 || matches!(op, 0x003c | 0x007c | 0x023c | 0x027c) {
        return false;
    }
    match family {
        "ADD" => top == 13 || op & 0xf100 == 0x5000 || op & 0xff00 == 0x0600,
        "SUB" => top == 9 || op & 0xf100 == 0x5100 || op & 0xff00 == 0x0400,
        "AND" => {
            top == 12 && (opmode <= 2 || opmode >= 4 && op >> 3 & 7 >= 2) || op & 0xff00 == 0x0200
        }
        "OR" => {
            top == 8 && (opmode <= 2 || opmode >= 4 && op >> 3 & 7 >= 2) || op & 0xff00 == 0x0000
        }
        _ => false,
    }
}

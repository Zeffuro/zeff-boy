pub(super) fn instruction_class(word: u16) -> bool {
    if word == 0x4e71
        || word & 0xf100 == 0x7000
        || word >> 12 == 6
        || super::tst::legal(word)
        || super::cmp::legal(word)
        || super::cmp::legal_address(word)
        || super::cmpi::legal(word)
        || super::status::legal(word)
        || super::from_sr::legal(word)
        || super::rmw::legal(word)
        || super::address_arithmetic::legal(word)
        || super::data_arithmetic::legal(word)
    {
        return true;
    }
    if !matches!(word >> 12, 1..=3) {
        return false;
    }
    let source_mode = (word >> 3) & 7;
    let source_register = word & 7;
    let destination_mode = (word >> 6) & 7;
    let destination_register = (word >> 9) & 7;
    !(source_mode == 7 && source_register > 4
        || destination_mode == 7 && destination_register > 1
        || word >> 12 == 1 && (source_mode == 1 || destination_mode == 1))
}

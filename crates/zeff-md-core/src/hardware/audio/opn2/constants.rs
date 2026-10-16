pub(super) const OPERATORS: usize = 4;
pub(super) const REGISTER_SLOTS: [usize; OPERATORS] = [0, 2, 1, 3];
pub(super) const PHASE_BITS: u32 = 20;
pub(super) const OUTPUT_PHASE_SHIFT: u32 = 1;
pub(super) const PHASE_MASK: u32 = (1 << PHASE_BITS) - 1;
pub(super) const DETUNE_MASK: i32 = (1 << 17) - 1;
pub(super) const DETUNE_MAGNITUDES: [[u8; 4]; 32] = [
    [0, 0, 1, 2],
    [0, 0, 1, 2],
    [0, 0, 1, 2],
    [0, 0, 1, 2],
    [0, 1, 2, 2],
    [0, 1, 2, 3],
    [0, 1, 2, 3],
    [0, 1, 2, 3],
    [0, 1, 2, 4],
    [0, 1, 3, 4],
    [0, 1, 3, 4],
    [0, 1, 3, 5],
    [0, 2, 4, 5],
    [0, 2, 4, 6],
    [0, 2, 4, 6],
    [0, 2, 5, 7],
    [0, 2, 5, 8],
    [0, 3, 6, 8],
    [0, 3, 6, 9],
    [0, 3, 7, 10],
    [0, 4, 8, 11],
    [0, 4, 8, 12],
    [0, 4, 9, 13],
    [0, 5, 10, 14],
    [0, 5, 11, 16],
    [0, 6, 12, 17],
    [0, 6, 13, 19],
    [0, 7, 14, 20],
    [0, 8, 16, 22],
    [0, 8, 16, 22],
    [0, 8, 16, 22],
    [0, 8, 16, 22],
];
pub(super) const SINE_BITS: u32 = 10;
pub(super) const SINE_QUARTER: usize = 1 << (SINE_BITS - 2);
pub(super) const OPERATOR_PEAK: i32 = 8191;
pub(super) const ENVELOPE_MAX: u16 = 1023;
pub(super) const ENVELOPE_DIVIDER: u8 = 3;
pub(super) const ENVELOPE_COUNTER_MAX: u16 = (1 << 12) - 1;
pub(super) const ENVELOPE_PATTERNS: [u8; 4] = [0xaa, 0xba, 0xee, 0xfe];
pub(super) const LFO_DIVIDERS: [u8; 8] = [108, 77, 71, 67, 62, 44, 8, 5];
pub(super) const LFO_AM_SHIFTS: [u8; 4] = [7, 3, 1, 0];
pub(super) const LFO_PM_WEIGHTS: [[u8; 8]; 6] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 1, 1, 1, 1],
    [0, 0, 0, 1, 1, 1, 2, 2],
    [0, 0, 1, 1, 2, 2, 3, 3],
    [0, 0, 1, 2, 2, 2, 3, 4],
    [0, 0, 2, 3, 4, 4, 5, 6],
];

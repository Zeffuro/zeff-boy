use super::super::Cue;

// Rows are raw id, frame count, clock count, optional loop frame, and bank sequence.
type CueRow = (u8, u32, u64, Option<u32>, &'static [u8]);

pub(super) const fn from_rows<const N: usize>(rows: [CueRow; N]) -> [Cue; N] {
    let mut output = [const {
        Cue {
            raw: 0,
            frames: 0,
            clocks: 0,
            loop_start: None,
            banks: &[],
        }
    }; N];
    let mut index = 0;
    while index < N {
        let (raw, frames, clocks, loop_start, banks) = rows[index];
        output[index] = Cue {
            raw,
            frames,
            clocks,
            loop_start,
            banks,
        };
        index += 1;
    }
    output
}

use super::*;

#[derive(Clone, Copy)]
pub(super) struct PlaneLayout {
    pub columns: usize,
    pub x_mask: u16,
    pub y_mask: usize,
    pub inspection_height: usize,
}

impl PlaneLayout {
    pub fn new(value: u8, interlaced: bool) -> Self {
        let horizontal = value & PLANE_SIZE_AXIS_MASK;
        let vertical = (value >> PLANE_HEIGHT_SHIFT) & PLANE_SIZE_AXIS_MASK;
        let columns = match horizontal {
            1 => PLANE_AXIS_MEDIUM,
            3 => PLANE_AXIS_LARGE,
            _ => PLANE_AXIS_SMALL,
        };
        let row_mask = if horizontal == 2 {
            0
        } else {
            ((usize::from(vertical) * PLANE_AXIS_SMALL) | (PLANE_AXIS_SMALL - 1))
                & (PLANE_MAX_CELLS / columns - 1)
        };
        let height = tile_height(interlaced);
        Self {
            columns,
            x_mask: (columns * TILE_WIDTH - 1) as u16,
            y_mask: (row_mask * height) | (height - 1),
            inspection_height: (row_mask + 1).next_power_of_two() * height,
        }
    }
}

use super::Mbc7;

pub(super) const ACCEL_CENTER: u16 = 0x81D0;
pub(super) const ACCEL_MAX_DELTA: i32 = 0x01A0;
pub(super) const ACCEL_LATCH_VALUE: u8 = 0x55;
pub(super) const ACCEL_RESET_VALUE: u8 = 0xAA;

const REG_LATCH: u8 = 0x0;
const REG_RESET: u8 = 0x1;
const REG_ACCEL_X_LO: u8 = 0x2;
const REG_ACCEL_X_HI: u8 = 0x3;
const REG_ACCEL_Y_LO: u8 = 0x4;
const REG_ACCEL_Y_HI: u8 = 0x5;

impl Mbc7 {
    pub(super) fn read_accel_register(&self, reg: u8) -> Option<u8> {
        match reg {
            REG_LATCH | REG_RESET | 0x6 | 0x7 => Some(0x00),
            REG_ACCEL_X_LO => Some(self.x_latch as u8),
            REG_ACCEL_X_HI => Some((self.x_latch >> 8) as u8),
            REG_ACCEL_Y_LO => Some(self.y_latch as u8),
            REG_ACCEL_Y_HI => Some((self.y_latch >> 8) as u8),
            _ => None,
        }
    }

    pub(super) fn write_accel_register(&mut self, reg: u8, value: u8) -> bool {
        match reg {
            REG_LATCH => {
                if value == ACCEL_LATCH_VALUE {
                    self.latch_ready = true;
                    self.x_latch = 0x8000;
                    self.y_latch = 0x8000;
                }
                true
            }
            REG_RESET => {
                if value == ACCEL_RESET_VALUE {
                    self.latch_ready = false;
                    self.x_latch = Self::map_tilt_component(-self.host_x);
                    self.y_latch = Self::map_tilt_component(self.host_y);
                }
                true
            }
            REG_ACCEL_X_LO | REG_ACCEL_X_HI | REG_ACCEL_Y_LO | REG_ACCEL_Y_HI | 0x6 | 0x7 => true,
            _ => false,
        }
    }

    fn map_tilt_component(value: f32) -> u16 {
        let clamped = value.clamp(-1.0, 1.0);
        let offset = (clamped * ACCEL_MAX_DELTA as f32).round() as i32;
        (ACCEL_CENTER as i32 + offset).clamp(0, u16::MAX as i32) as u16
    }
}

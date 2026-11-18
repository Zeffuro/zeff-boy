use super::Guest;

impl Guest {
    pub fn send_held(&mut self, byte: u8) -> usize {
        self.send_inner(byte, true)
    }
}

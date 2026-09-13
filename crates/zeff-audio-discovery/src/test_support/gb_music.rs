use crate::gb_music::GbSong;

pub fn synthetic_song(bytes: &[u8], bank: u8, address: u16) -> GbSong {
    crate::gb_music::synthetic_song_for_test_support(bytes, bank, address)
}

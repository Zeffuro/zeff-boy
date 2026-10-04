use anyhow::{Result, ensure};

pub const CHAT_MAX_BYTES: usize = 512;

pub fn validate_chat(text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), "Enter a message.");
    ensure!(
        text.len() <= CHAT_MAX_BYTES,
        "Message is too long (512 bytes max)."
    );
    ensure!(
        !text.chars().any(|ch| ch.is_control() || matches!(ch, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{061c}' | '\u{200e}' | '\u{200f}')),
        "Message contains unsupported characters."
    );
    Ok(())
}

const NES_GG_ALPHABET: &[u8; 16] = b"APZLGITYEOXUKSVN";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NesGameGeniePatch {
    pub address: u16,
    pub value: u8,
    pub compare: Option<u8>,
}

fn gg_letter_to_value(c: char) -> Option<u8> {
    let upper = c.to_ascii_uppercase() as u8;
    NES_GG_ALPHABET
        .iter()
        .position(|&ch| ch == upper)
        .map(|i| i as u8)
}

pub fn decode_nes_game_genie(code: &str) -> Option<NesGameGeniePatch> {
    let cleaned: String = code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();

    let n: Vec<u8> = cleaned.chars().map(gg_letter_to_value).collect::<Option<_>>()?;

    let decode_address = |n: &[u8]| -> u16 {
        0x8000
            | u16::from(n[4] & 7)
            | u16::from(n[2] & 8)
            | (u16::from(n[2] & 7) << 4)
            | (u16::from(n[4] & 8) << 4)
            | (u16::from(n[5] & 7) << 8)
            | (u16::from(n[1] & 8) << 8)
            | (u16::from(n[3] & 7) << 12)
    };

    match n.len() {
        6 => {
            let value = (n[1] & 7)
                | (n[5] & 8)
                | ((n[0] & 7) << 4)
                | ((n[0] & 8) << 4);

            let address = decode_address(&n);

            Some(NesGameGeniePatch {
                address,
                value,
                compare: None,
            })
        }
        8 => {
            let value = (n[1] & 7)
                | (n[7] & 8)
                | ((n[0] & 7) << 4)
                | ((n[0] & 8) << 4);

            let compare = (n[7] & 7)
                | (n[5] & 8)
                | ((n[6] & 7) << 4)
                | ((n[6] & 8) << 4);

            let address = decode_address(&n);

            Some(NesGameGeniePatch {
                address,
                value,
                compare: Some(compare),
            })
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct NesCheatState {
    pub patches: Vec<NesGameGeniePatch>,
}

impl NesCheatState {
    pub fn new() -> Self {
        Self {
            patches: Vec::new(),
        }
    }

    #[inline]
    pub fn intercept(&self, address: u16, original: u8) -> Option<u8> {
        if self.patches.is_empty() {
            return None;
        }
        for patch in &self.patches {
            if patch.address == address {
                match patch.compare {
                    Some(cmp) if cmp != original => continue,
                    _ => return Some(patch.value),
                }
            }
        }
        None
    }

    pub fn clear(&mut self) {
        self.patches.clear();
    }
}

impl Default for NesCheatState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

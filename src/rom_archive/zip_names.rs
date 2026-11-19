use std::borrow::Cow;
use std::io::Read;
use std::path::PathBuf;
use zip::read::ZipFile;

const UTF8_FLAG: u16 = 1 << 11;
const CP437: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ',
    'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ',
    'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕',
    '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦',
    '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐',
    '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±',
    '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{a0}',
];

fn decode_cp437(raw: &[u8]) -> Cow<'_, str> {
    if raw.is_ascii() {
        return String::from_utf8_lossy(raw);
    }
    Cow::Owned(
        raw.iter()
            .map(|&byte| {
                if byte.is_ascii() {
                    char::from(byte)
                } else {
                    CP437[usize::from(byte - 128)]
                }
            })
            .collect(),
    )
}

pub(crate) fn decode_zip_member_name<'a>(entry: &'a ZipFile<'_, impl Read>) -> Cow<'a, str> {
    // Persisted ROM paths depend on the ZIP language flag, even for CP437 bytes that form UTF-8.
    if entry.flags().as_u16() & UTF8_FLAG != 0 {
        String::from_utf8_lossy(entry.name_raw())
    } else {
        decode_cp437(entry.name_raw())
    }
}

pub(crate) fn enclosed_zip_member_path(entry: &ZipFile<'_, impl Read>) -> Option<PathBuf> {
    let enclosed = entry.enclosed_name()?;
    if entry.flags().as_u16() & UTF8_FLAG == 0
        && !entry.name_raw().is_ascii()
        && std::str::from_utf8(entry.name_raw()).is_ok()
    {
        return enclosed
            .iter()
            .map(|component| component.to_str().map(|text| decode_cp437(text.as_bytes())))
            .map(|component| component.map(|text| PathBuf::from(text.as_ref())))
            .collect();
    }
    Some(enclosed)
}

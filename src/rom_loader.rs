use std::fs::File;
use std::io::Read;
use std::path::Path;

pub(crate) fn load_rom(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer)?;
    Ok(buffer)
}

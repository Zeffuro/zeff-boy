#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum HardwareMode {
    DMG,
    SGB1,
    SGB2,
    CGBNormal,
    CGBDouble,
}
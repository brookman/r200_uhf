use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Frame(#[from] FrameError),

    #[error("command error: {0}")]
    Command(#[from] CommandError),

    #[error("invalid payload: {0}")]
    InvalidPayload(String),
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame too short ({0} bytes, minimum 7)")]
    TooShort(usize),

    #[error("bad header byte: expected 0xAA, got 0x{0:02X}")]
    BadHeader(u8),

    #[error("unknown frame type: 0x{0:02X}")]
    UnknownFrameType(u8),

    #[error("checksum mismatch: expected 0x{expected:02X}, got 0x{actual:02X}")]
    ChecksumMismatch { expected: u8, actual: u8 },

    #[error("truncated frame: declared length {declared} but got {available} data bytes")]
    Truncated { declared: usize, available: usize },

    #[error("missing frame end marker: expected 0xDD, got 0x{0:02X}")]
    MissingEndMarker(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandError(pub u8);

impl CommandError {
    pub const SUCCESS: CommandError = CommandError(0x00);

    pub fn is_success(self) -> bool {
        self.0 == 0x00
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            0x00 => write!(f, "success"),
            0x01 => write!(f, "inventory failure"),
            0x02 => write!(f, "access failure"),
            0x03 => write!(f, "command error"),
            0x04 => write!(f, "FHSS failure"),
            0x05 => write!(f, "custom region failure"),
            0x06 => write!(f, "power failure"),
            0x07 => write!(f, "BAP calibration data failure"),
            0x08 => write!(f, "tag lost"),
            0x09 => write!(f, "read failure"),
            0x0A => write!(f, "write failure"),
            0x0B => write!(f, "kill failure"),
            0x0C => write!(f, "lock failure"),
            0x80 => write!(f, "not initialized"),
            b @ 0xA0..=0xEF => write!(f, "tag access error (0x{b:02X})"),
            other => write!(f, "unknown error (0x{other:02X})"),
        }
    }
}

impl std::error::Error for CommandError {}

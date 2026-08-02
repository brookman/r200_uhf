use std::fmt::{Display, Formatter};

/// Known R200 constants
pub const R200_FRAME_HEADER: u8 = 0xAA;
pub const R200_FRAME_END: u8 = 0xDD;

/// Frame type:
const FRAME_TYPE_SEND_COMMAND: u8 = 0x00; // from PC to R200
const INSTRUCTION_READER_WRITER_MODULE_INFO: u8 = 0x03; // Get reader/writer module information

#[derive(Debug)]
pub enum FrameError {
    InvalidCommand(String),
}

impl Display for FrameError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::InvalidCommand(msg) => write!(f, "Invalid command: {}", msg),
        }
    }
}

impl std::error::Error for FrameError {}

pub enum Command {
    GetWorkingChannel,
    GetWorkingArea,
    SetWorkingArea(u8),
    AcquireTransmitPower,
    SetTransmissionPower(f64),
    HardwareVersion,
    SoftwareVersion,
    Manufacturer,
    SinglePollingInstruction,
    MultiplePollingInstruction(u16),
    StopMultiplePollingInstruction,
    SetSelect(Vec<u8>),
    ReadLabel(Vec<u8>),
    WriteLabel(Vec<u8>),
    KillTag(Vec<u8>),
    LockTag(Vec<u8>),
}

/// M100 protocol error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Success,
    CommandError,
    FhssFail,
    InventoryFail,
    AccessFail,
    ReadFail,
    ReadError(u8),
    WriteFail,
    WriteError(u8),
    LockFail,
    LockError(u8),
    KillFail,
    KillError(u8),
    BlockPermalockFail,
    BlockPermalockError(u8),
    ChangeConfigFail,
    ReadProtectFail,
    ResetReadProtectFail,
    ChangeEasFail,
    EasAlarmFail,
    QtFail,
    Unknown(u8),
}

impl ErrorCode {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => ErrorCode::Success,
            0x09 => ErrorCode::ReadFail,
            0x10 => ErrorCode::WriteFail,
            0x12 => ErrorCode::KillFail,
            0x13 => ErrorCode::LockFail,
            0x14 => ErrorCode::BlockPermalockFail,
            0x15 => ErrorCode::InventoryFail,
            0x16 => ErrorCode::AccessFail,
            0x17 => ErrorCode::CommandError,
            0x1A => ErrorCode::ChangeConfigFail,
            0x1B => ErrorCode::ChangeEasFail,
            0x1D => ErrorCode::EasAlarmFail,
            0x20 => ErrorCode::FhssFail,
            0x2A => ErrorCode::ReadProtectFail,
            0x2B => ErrorCode::ResetReadProtectFail,
            0x2E => ErrorCode::QtFail,
            b if b & 0xF0 == 0xA0 => ErrorCode::ReadError(b & 0x0F),
            b if b & 0xF0 == 0xB0 => ErrorCode::WriteError(b & 0x0F),
            b if b & 0xF0 == 0xC0 => ErrorCode::LockError(b & 0x0F),
            b if b & 0xF0 == 0xD0 => ErrorCode::KillError(b & 0x0F),
            b if b & 0xF0 == 0xE0 => ErrorCode::BlockPermalockError(b & 0x0F),
            _ => ErrorCode::Unknown(b),
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCode::Success => write!(f, "Success"),
            ErrorCode::CommandError => write!(f, "Command error (0x17)"),
            ErrorCode::FhssFail => write!(f, "FHSS fail (0x20)"),
            ErrorCode::InventoryFail => write!(f, "Inventory fail - no tag or CRC error (0x15)"),
            ErrorCode::AccessFail => write!(f, "Access failed - wrong password (0x16)"),
            ErrorCode::ReadFail => write!(f, "Read failed - no tag response or CRC error (0x09)"),
            ErrorCode::ReadError(code) => write!(f, "Read error (0xA0|0x{:02X})", code),
            ErrorCode::WriteFail => write!(f, "Write failed - no tag response or CRC error (0x10)"),
            ErrorCode::WriteError(code) => write!(f, "Write error (0xB0|0x{:02X})", code),
            ErrorCode::LockFail => write!(f, "Lock failed (0x13)"),
            ErrorCode::LockError(code) => write!(f, "Lock error (0xC0|0x{:02X})", code),
            ErrorCode::KillFail => write!(f, "Kill failed (0x12)"),
            ErrorCode::KillError(code) => write!(f, "Kill error (0xD0|0x{:02X})", code),
            ErrorCode::BlockPermalockFail => write!(f, "BlockPermalock failed (0x14)"),
            ErrorCode::BlockPermalockError(code) => {
                write!(f, "BlockPermalock error (0xE0|0x{:02X})", code)
            }
            ErrorCode::ChangeConfigFail => write!(f, "ChangeConfig failed (0x1A)"),
            ErrorCode::ReadProtectFail => write!(f, "ReadProtect failed (0x2A)"),
            ErrorCode::ResetReadProtectFail => write!(f, "ResetReadProtect failed (0x2B)"),
            ErrorCode::ChangeEasFail => write!(f, "ChangeEAS failed (0x1B)"),
            ErrorCode::EasAlarmFail => write!(f, "EAS Alarm failed (0x1D)"),
            ErrorCode::QtFail => write!(f, "QT failed (0x2E)"),
            ErrorCode::Unknown(b) => write!(f, "Unknown error (0x{:02X})", b),
        }
    }
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::HardwareVersion => write!(f, "Hardware Version"),
            Command::SoftwareVersion => write!(f, "Software Version"),
            Command::Manufacturer => write!(f, "Manufacturer"),
            Command::GetWorkingChannel => write!(f, "Get Working Channel"),
            Command::GetWorkingArea => write!(f, "Get Working Area"),
            Command::AcquireTransmitPower => write!(f, "Acquire transmit power"),
            Command::SetTransmissionPower(power) => {
                write!(f, "Set transmission power to {}", power)
            }
            Command::SinglePollingInstruction => write!(f, "Single Polling Instruction"),
            Command::MultiplePollingInstruction(max) => {
                write!(f, "Multiple Polling Instruction [max: {max} times]")
            }
            Command::StopMultiplePollingInstruction => {
                write!(f, "Stop Multiple Polling Instruction")
            }
            Command::SetWorkingArea(code) => write!(f, "Set Working Area to {}", code),
            Command::SetSelect(params) => write!(f, "Set Select ({} bytes)", params.len()),
            Command::ReadLabel(params) => write!(f, "Read Label ({} bytes)", params.len()),
            Command::WriteLabel(params) => write!(f, "Write Label ({} bytes)", params.len()),
            Command::KillTag(params) => write!(f, "Kill Tag ({} bytes)", params.len()),
            Command::LockTag(params) => write!(f, "Lock Tag ({} bytes)", params.len()),
        }
    }
}

/// Trait for serializable commands
pub(crate) trait SerializableCommand {
    /// Returns a tuple of bytes (command, parameters)
    /// Parameters may be empty if not present
    fn to_bytes(&self) -> (Vec<u8>, Vec<u8>);
    fn from_tuple(tuple: (Vec<u8>, Vec<u8>)) -> Result<Self, FrameError>
    where
        Self: Sized;
}

const READ_WRITE_INFO_HARDWARE_VERSION: u8 = 0x00;
const READ_WRITE_INFO_SOFTWARE_VERSION: u8 = 0x01;
const READ_WRITE_INFO_MANUFACTURER: u8 = 0x02;

impl SerializableCommand for Command {
    fn to_bytes(&self) -> (Vec<u8>, Vec<u8>) {
        match self {
            Command::HardwareVersion => (
                vec![INSTRUCTION_READER_WRITER_MODULE_INFO],
                vec![READ_WRITE_INFO_HARDWARE_VERSION],
            ), //Command::HardwareVersion
            Command::SoftwareVersion => (
                vec![INSTRUCTION_READER_WRITER_MODULE_INFO],
                vec![READ_WRITE_INFO_SOFTWARE_VERSION],
            ), //Command::SoftwareVersion
            Command::Manufacturer => (
                vec![INSTRUCTION_READER_WRITER_MODULE_INFO],
                vec![READ_WRITE_INFO_MANUFACTURER],
            ), //Command::Manufacturer
            Command::GetWorkingChannel => (vec![0xAA], vec![]),
            Command::GetWorkingArea => (vec![0x08], vec![]),
            Command::AcquireTransmitPower => (vec![0xB7], vec![]),
            Command::SetTransmissionPower(p) => {
                let power = (p * 100.0) as u16;
                let mut v = Vec::new();
                v.push((power >> 8) as u8);
                v.push((power & 0xFF) as u8);
                (vec![0xB6], v)
            }
            Command::SinglePollingInstruction => (vec![0x22], vec![]),
            Command::MultiplePollingInstruction(max) => {
                let mut v = Vec::new();
                v.push((max >> 8) as u8);
                v.push((max & 0xFF) as u8);
                (vec![0x27], v)
            }
            Command::StopMultiplePollingInstruction => (vec![0x28], vec![]),
            Command::SetWorkingArea(code) => (vec![0x07], vec![*code]),
            Command::SetSelect(params) => (vec![0x0C], params.to_vec()),
            Command::ReadLabel(params) => (vec![0x39], params.to_vec()),
            Command::WriteLabel(params) => (vec![0x49], params.to_vec()),
            Command::KillTag(params) => (vec![0x65], params.to_vec()),
            Command::LockTag(params) => (vec![0x82], params.to_vec()),
        }
    }

    fn from_tuple(tuple: (Vec<u8>, Vec<u8>)) -> Result<Self, FrameError> {
        match (tuple.0[0], tuple.1[0]) {
            (INSTRUCTION_READER_WRITER_MODULE_INFO, READ_WRITE_INFO_HARDWARE_VERSION) => {
                Ok(Command::HardwareVersion)
            }
            (INSTRUCTION_READER_WRITER_MODULE_INFO, READ_WRITE_INFO_SOFTWARE_VERSION) => {
                Ok(Command::SoftwareVersion)
            }
            (INSTRUCTION_READER_WRITER_MODULE_INFO, READ_WRITE_INFO_MANUFACTURER) => {
                Ok(Command::Manufacturer)
            }
            (INSTRUCTION_READER_WRITER_MODULE_INFO, _) => Err(FrameError::InvalidCommand(format!(
                "Invalid command code: {}",
                tuple.1[0]
            ))),
            (0xAA, _) => Ok(Command::GetWorkingChannel),
            (0x08, _) => Ok(Command::GetWorkingArea),
            (0xB7, _) => Ok(Command::AcquireTransmitPower),
            (0x28, _) => Ok(Command::StopMultiplePollingInstruction),
            (0x07, code) => Ok(Command::SetWorkingArea(code)),
            _ => Err(FrameError::InvalidCommand(format!(
                "Invalid command code: {}",
                tuple.0[0]
            ))),
        }
    }
}

pub(crate) struct Frame {
    payload: Vec<u8>,
}

impl Frame {
    pub(crate) fn new(payload: &Command) -> Self {
        let mut v = Vec::new();
        // command
        v.extend(payload.to_bytes().0);
        let payload_size = payload.to_bytes().1.len() as u16;
        v.push((payload_size >> 8) as u8);
        v.push((payload_size & 0xFF) as u8);
        v.extend(payload.to_bytes().1);

        Frame { payload: v }
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.push(R200_FRAME_HEADER);
        v.push(FRAME_TYPE_SEND_COMMAND);

        v.extend(&self.payload);

        v.push(self.checksum(&v[2..]));
        v.push(R200_FRAME_END);
        v
    }

    fn checksum(&self, bytes: &[u8]) -> u8 {
        let sum: u16 = bytes.iter().map(|&b| b as u16).sum();
        (sum & 0xFF) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_bytes(cmd: Command) -> Vec<u8> {
        Frame::new(&cmd).to_bytes()
    }

    #[test]
    fn hardware_version_frame_bytes() {
        let bytes = frame_bytes(Command::HardwareVersion);
        let expected = vec![0xAA, 0x00, 0x03, 0x00, 0x01, 0x00, 0x04, 0xDD];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn software_version_frame_bytes() {
        let bytes = frame_bytes(Command::SoftwareVersion);
        let expected = vec![0xAA, 0x00, 0x03, 0x00, 0x01, 0x01, 0x05, 0xDD];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn manufacturer_frame_bytes() {
        let bytes = frame_bytes(Command::Manufacturer);
        let expected = vec![0xAA, 0x00, 0x03, 0x00, 0x01, 0x02, 0x06, 0xDD];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn get_working_channel_frame_bytes() {
        let bytes = frame_bytes(Command::GetWorkingChannel);
        let expected = vec![0xAA, 0x00, 0xAA, 0x00, 0x00, 0xAA, 0xDD];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn single_polling_instruction_frame_bytes() {
        let bytes = frame_bytes(Command::SinglePollingInstruction);
        let expected = vec![0xAA, 0x00, 0x22, 0x00, 0x00, 0x22, 0xDD];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn acquire_transmit_power_frame_bytes() {
        let bytes = frame_bytes(Command::AcquireTransmitPower);
        let expected = vec![0xAA, 0x00, 0xB7, 0x00, 0x00, 0xB7, 0xDD];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn set_transmission_power_frame_bytes() {
        // 26.50 dBm -> 2650 -> 0x0A 0x5A
        let bytes = frame_bytes(Command::SetTransmissionPower(26.50));
        let expected = vec![0xAA, 0x00, 0xB6, 0x00, 0x02, 0x0A, 0x5A, 0x1C, 0xDD];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn kill_tag_frame_bytes() {
        // Kill: command 0x65, params = 4-byte kill password 0000FFFF
        let bytes = frame_bytes(Command::KillTag(vec![0x00, 0x00, 0xFF, 0xFF]));
        let expected = vec![
            0xAA, 0x00, 0x65, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xFF, 0x67, 0xDD,
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn lock_tag_frame_bytes() {
        // Lock: command 0x82, params = 4-byte access password + 3-byte lock data (LD)
        let bytes = frame_bytes(Command::LockTag(vec![
            0x00, 0x00, 0xFF, 0xFF, 0x02, 0x00, 0x80,
        ]));
        let expected = vec![
            0xAA, 0x00, 0x82, 0x00, 0x07, 0x00, 0x00, 0xFF, 0xFF, 0x02, 0x00, 0x80, 0x09, 0xDD,
        ];
        assert_eq!(bytes, expected);
    }

    #[test]
    fn serializable_command_to_bytes_and_from_tuple() {
        // to_bytes
        assert_eq!(
            Command::HardwareVersion.to_bytes(),
            (vec![0x03], vec![0x00])
        );
        assert_eq!(
            Command::SoftwareVersion.to_bytes(),
            (vec![0x03], vec![0x01])
        );
        assert_eq!(Command::Manufacturer.to_bytes(), (vec![0x03], vec![0x02]));
        assert_eq!(Command::GetWorkingChannel.to_bytes(), (vec![0xAA], vec![]));
        assert_eq!(Command::GetWorkingArea.to_bytes(), (vec![0x08], vec![]));
        assert_eq!(
            Command::AcquireTransmitPower.to_bytes(),
            (vec![0xB7], vec![])
        );

        let (cmd, params) = Command::SetTransmissionPower(26.5).to_bytes();
        assert_eq!(cmd, vec![0xB6]);
        assert_eq!(params, vec![0x0A, 0x5A]); // 26.5 dBm -> 2650 -> 0x0A 0x5A

        assert_eq!(
            Command::KillTag(vec![0x00, 0x00, 0xFF, 0xFF]).to_bytes(),
            (vec![0x65], vec![0x00, 0x00, 0xFF, 0xFF])
        );
        assert_eq!(
            Command::LockTag(vec![0x00, 0x00, 0xFF, 0xFF, 0x02, 0x00, 0x80]).to_bytes(),
            (vec![0x82], vec![0x00, 0x00, 0xFF, 0xFF, 0x02, 0x00, 0x80])
        );

        // from_tuple
        assert!(matches!(
            Command::from_tuple((vec![0x03], vec![0x00])),
            Ok(Command::HardwareVersion)
        ));
        assert!(matches!(
            Command::from_tuple((vec![0x03], vec![0x01])),
            Ok(Command::SoftwareVersion)
        ));
        assert!(matches!(
            Command::from_tuple((vec![0x03], vec![0x02])),
            Ok(Command::Manufacturer)
        ));
        assert!(matches!(
            Command::from_tuple((vec![0xAA], vec![0x00])),
            Ok(Command::GetWorkingChannel)
        ));
        assert!(matches!(
            Command::from_tuple((vec![0x08], vec![0x00])),
            Ok(Command::GetWorkingArea)
        ));
        assert!(matches!(
            Command::from_tuple((vec![0xB7], vec![0x00])),
            Ok(Command::AcquireTransmitPower)
        ));
    }

    #[test]
    fn from_tuple_invalid_command_errors() {
        // Unknown subcode for module info
        let err = Command::from_tuple((vec![0x03], vec![0xFF]))
            .err()
            .expect("expected error");
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid command"));

        // Unknown main code
        let err = Command::from_tuple((vec![0x99], vec![0x00]))
            .err()
            .expect("expected error");
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid command"));
    }
}

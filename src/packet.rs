use crate::frame::SerializableCommand;
use crate::frame::{Command, FrameError};
use std::fmt::Display;

/// A raw frame received from the reader, with helpers to decode its fields.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Packet {
    raw_data: Vec<u8>,
}

impl Packet {
    pub(crate) fn new(raw_data: Vec<u8>) -> Packet {
        Packet { raw_data }
    }
    fn frame_type(&self) -> u8 {
        self.raw_data[1]
    }
    fn command_code(&self) -> u8 {
        self.raw_data[2]
    }
    fn data_len(&self) -> u16 {
        ((self.raw_data[3] as u16) << 8) | (self.raw_data[4] as u16)
    }

    pub(crate) fn get_data(&self) -> Vec<u8> {
        let data = &self.raw_data[5..(5 + self.data_len() as usize)];
        data.to_vec()
    }

    /// Returns true if this packet is an error response (command code 0xFF).
    pub fn is_error(&self) -> bool {
        self.command_code() == 0xFF
    }

    /// If this is an error packet, returns the M100 error code byte.
    pub fn error_code_byte(&self) -> Option<u8> {
        if self.is_error() {
            self.get_data().first().copied()
        } else {
            None
        }
    }

    /// Check if packet is valid
    pub fn is_valid(&self) -> bool {
        // If length is incorrect with what is expected
        if 5 + 2 + self.data_len() as usize != self.raw_data.len() {
            return false;
        }
        // Validate checksum: sum of bytes from index 1 (type) to second-to-last byte
        let cs_pos = self.raw_data.len() - 2;
        let sum: u16 = self.raw_data[1..cs_pos].iter().map(|&b| b as u16).sum();
        if (sum & 0xFF) as u8 != self.raw_data[cs_pos] {
            return false;
        }
        true
    }

    pub(crate) fn debug(&self) -> String {
        format!(
            "Type: {:02X}, Command: {:02X}, Length: {} - Data: {:?}",
            self.frame_type(),
            self.command_code(),
            self.data_len(),
            self.get_data()
        )
    }

    pub(crate) fn command(&self) -> Result<Command, FrameError> {
        Command::from_tuple((vec![self.command_code()], vec![self.raw_data[5]]))
    }
}

impl Display for Packet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let out = {
            if let Ok(text) = std::str::from_utf8(&self.get_data()) {
                text.to_string()
            } else {
                "Invalid UTF-8".to_string()
            }
        };
        write!(f, "{out}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to build a raw packet vector: [HEADER, TYPE, CMD, LEN_HI, LEN_LO, DATA..., CHECKSUM, END]
    fn build_packet(frame_type: u8, cmd: u8, data: &[u8]) -> Vec<u8> {
        let len = data.len() as u16;
        let mut v = vec![
            crate::frame::R200_FRAME_HEADER,
            frame_type,
            cmd,
            (len >> 8) as u8,
            (len & 0xFF) as u8,
        ];
        v.extend_from_slice(data);
        // checksum is sum of bytes from index 1 (type) to last data byte, low 8 bits
        let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
        v.push((sum & 0xFF) as u8);
        v.push(crate::frame::R200_FRAME_END);
        v
    }

    #[test]
    fn packet_parses_basic_fields() {
        let raw = build_packet(0x00, 0x03, &[0x00]); // module info, hardware version parameter
        let p = Packet::new(raw.clone());
        assert_eq!(p.frame_type(), 0x00);
        assert_eq!(p.command_code(), 0x03);
        assert_eq!(p.data_len(), 1);
        assert_eq!(p.get_data(), vec![0x00]);
        // debug string should contain hex codes and length
        let dbg = p.debug();
        assert!(dbg.contains("Type: 00"));
        assert!(dbg.contains("Command: 03"));
        assert!(dbg.contains("Length: 1"));
    }

    #[test]
    fn display_outputs_utf8_data() {
        let raw = build_packet(0x00, 0x22, b"OK");
        let p = Packet::new(raw);
        assert_eq!(format!("{}", p), "OK");
    }

    #[test]
    fn display_handles_invalid_utf8() {
        let raw = build_packet(0x00, 0x22, &[0xFF]);
        let p = Packet::new(raw);
        assert_eq!(format!("{}", p), "Invalid UTF-8");
    }

    #[test]
    fn command_mapping_module_info_variants() {
        // HardwareVersion (0x03, 0x00)
        let p_hw = Packet::new(build_packet(0x00, 0x03, &[0x00]));
        assert!(matches!(p_hw.command().unwrap(), Command::HardwareVersion));
        // SoftwareVersion (0x03, 0x01)
        let p_sw = Packet::new(build_packet(0x00, 0x03, &[0x01]));
        assert!(matches!(p_sw.command().unwrap(), Command::SoftwareVersion));
        // Manufacturer (0x03, 0x02)
        let p_mf = Packet::new(build_packet(0x00, 0x03, &[0x02]));
        assert!(matches!(p_mf.command().unwrap(), Command::Manufacturer));
    }

    #[test]
    fn command_mapping_other_commands_with_no_data() {
        // GetWorkingChannel uses 0xAA with no data length
        let raw = build_packet(0x00, 0xAA, &[]);
        let p = Packet::new(raw);
        // Our implementation looks at raw_data[5] even when len=0, which is checksum.
        // Command::from_tuple ignores the second element for these commands, so this should still work.
        assert!(matches!(p.command().unwrap(), Command::GetWorkingChannel));
    }

    #[test]
    fn packet_validity() {
        let correct_bytes = build_packet(0x00, 0x03, &[0x00, 0x01, 0x02]);

        let p = Packet::new(correct_bytes.clone());
        assert!(p.is_valid());

        let mut incorrect_bytes = correct_bytes.clone();
        incorrect_bytes[4] = 0x10;
        let p = Packet::new(incorrect_bytes);
        assert!(!p.is_valid());
    }

    #[test]
    fn error_packet_detection() {
        let err = Packet::new(build_packet(0x00, 0xFF, &[0x10])); // WriteFail
        assert!(err.is_error());
        assert_eq!(err.error_code_byte(), Some(0x10));

        let ok = Packet::new(build_packet(0x00, 0x22, &[])); // normal response
        assert!(!ok.is_error());
        assert_eq!(ok.error_code_byte(), None);

        // Empty error data: no code byte to extract
        let empty = Packet::new(build_packet(0x00, 0xFF, &[]));
        assert!(empty.is_error());
        assert_eq!(empty.error_code_byte(), None);
    }

    #[test]
    fn command_mapping_set_working_area() {
        let p = Packet::new(build_packet(0x00, 0x07, &[0x03])); // EU = 3
        assert!(matches!(p.command().unwrap(), Command::SetWorkingArea(3)));
    }
}

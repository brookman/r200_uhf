use std::fmt::Display;
use std::hash::Hash;

/// A tag detected during an inventory, with its radio and identifier fields.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub struct Rfid {
    /// Received signal strength indicator from the reader.
    pub rssi: u8,
    /// Protocol-control word of the tag, as hex.
    pub pc: String,
    /// EPC of the tag, as hex (also known as the tag UID).
    pub epc: String,
    /// CRC of the tag response, as hex.
    pub crc: String,
    pub(crate) raw: Vec<u8>,
}

impl Rfid {
    pub(crate) fn from_raw(raw: Vec<u8>) -> Rfid {
        let rssi = raw[0];

        Self {
            pc: bytes_to_hex_upper(&raw[1..3]),
            epc: bytes_to_hex_upper(&raw[3..15]),
            crc: bytes_to_hex_upper(&raw[15..17]),
            rssi,
            raw,
        }
    }
}

impl Hash for Rfid {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.epc.hash(state);
    }
}

impl PartialEq<Self> for Rfid {
    fn eq(&self, other: &Self) -> bool {
        self.epc == other.epc
    }
}
impl Eq for Rfid {}

impl Display for Rfid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RSSI: {}, PC: {}, EPC(UID): {:?}, CRC: {}, RAW: {}",
            self.rssi,
            self.pc,
            self.epc,
            self.crc,
            bytes_to_hex_upper(&self.raw)
        )
    }
}

impl Rfid {
    /// The tag UID (EPC) as an uppercase hex string.
    pub fn uid(&self) -> String {
        self.epc.clone()
    }
}

fn bytes_to_hex_upper(bytes: &[u8]) -> String {
    // Use manual formatting for performance / control.
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02X}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing_rfid() {
        let intake = "BC3000E28069150000501D63E2784FB0B7";

        let bytes: Vec<u8> = (0..intake.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&intake[i..i + 2], 16).unwrap())
            .collect();

        let packet = Rfid::from_raw(bytes);

        assert_eq!(packet.rssi, 0xBC);
        assert_eq!(packet.pc, "3000");
        assert_eq!(packet.epc, "E28069150000501D63E2784F");
        assert_eq!(packet.crc, "B0B7");
    }

    #[test]
    fn uid_matches_epc() {
        let intake = "BC3000E28069150000501D63E2784FB0B7";
        let bytes: Vec<u8> = (0..intake.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&intake[i..i + 2], 16).unwrap())
            .collect();

        let packet = Rfid::from_raw(bytes);
        assert_eq!(packet.uid(), "E28069150000501D63E2784F");
    }
}

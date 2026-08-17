use crate::Tag;

pub fn display_tag(tag: &Tag) {
    println!(
        "  EPC: {}  RSSI: {}  PC: {}  CRC: {}",
        tag.epc_hex(),
        tag.rssi,
        tag.pc_hex(),
        tag.crc_hex(),
    );
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

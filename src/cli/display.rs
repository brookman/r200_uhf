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
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

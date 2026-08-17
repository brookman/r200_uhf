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

pub fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() % 2 != 0 {
        return Err("hex string must have even length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|e| e.to_string())
}

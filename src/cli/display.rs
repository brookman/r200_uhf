use crate::Rfid;

/// Render bytes as a lowercase hex string.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Parse a hex EPC string into raw bytes.
pub fn epc_bytes(epc_hex: &str) -> Vec<u8> {
    (0..epc_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&epc_hex[i..i + 2], 16).unwrap_or(0))
        .collect()
}

/// Print a human-readable description of a detected tag.
pub fn display_tag(tag: &Rfid) {
    println!("  EPC:     {}  (RSSI {})", tag.epc, tag.rssi);
    let bytes = epc_bytes(&tag.epc);
    match gs1::epc::decode_binary(&bytes) {
        Ok(epc) => {
            println!("  URI:     {}", epc.to_uri());
            if let gs1::epc::EPCValue::SGTIN96(sgtin) = epc.get_value() {
                let company_str = format!(
                    "{:0width$}",
                    sgtin.gtin.company,
                    width = sgtin.gtin.company_digits
                );
                let item_str = format!(
                    "{:0width$}",
                    sgtin.gtin.item,
                    width = 13 - sgtin.gtin.company_digits - 1
                );
                let gtin_str = format!("{}{}{}", sgtin.gtin.indicator, company_str, item_str);
                println!("  GTIN:    {}", gtin_str);
                println!("  Company: {}", company_str);
                println!("  Item:    {}", item_str);
                println!("  Serial:  {}", sgtin.serial);
            }
        }
        Err(_) => {
            println!("  PC:      {}", tag.pc);
            println!("  CRC:     {}", tag.crc);
        }
    }
}

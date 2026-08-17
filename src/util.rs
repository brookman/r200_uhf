pub trait PushU16 {
    fn push_u16(&mut self, val: u16);
}

impl PushU16 for Vec<u8> {
    fn push_u16(&mut self, val: u16) {
        self.push((val >> 8) as u8);
        self.push((val & 0xFF) as u8);
    }
}

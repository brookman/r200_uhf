use std::io::{Read, Write};
use std::time::Duration;

use crate::core::command::Command;
use crate::core::error::CoreError;
use crate::core::frame::{Frame, FRAME_HEADER};

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);

pub struct SyncReader<W> {
    port: W,
    read_buf: Vec<u8>,
    timeout: Duration,
}

impl<W: Read + Write> SyncReader<W> {
    pub fn new(port: W) -> Self {
        Self {
            port,
            read_buf: Vec::with_capacity(4096),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn send<C: Command>(&mut self, cmd: &C) -> Result<C::Response, CoreError> {
        let wire = Frame::encode_command(C::CODE, &cmd.encode());
        log::trace!("send {:02X}: {:02X?}", C::CODE, wire);
        self.port.write_all(&wire)?;
        self.port.flush()?;

        let frame = self.read_frame()?;
        log::trace!("recv {:02X}: {:02X?}", frame.command_code, frame.data);

        if frame.command_code == 0xFF {
            let err_code = frame.data.first().copied().unwrap_or(0xFF);
            return Err(CoreError::Command(crate::core::error::CommandError(err_code)));
        }

        Ok(cmd.decode_response(&frame.data)?)
    }

    fn read_frame(&mut self) -> Result<Frame, CoreError> {
        let mut tmp = [0u8; 1024];
        let deadline = std::time::Instant::now() + self.timeout;

        loop {
            // Try to decode from existing buffer first
            while let Some(pos) = self.read_buf.iter().position(|&b| b == FRAME_HEADER) {
                if pos > 0 {
                    self.read_buf.drain(..pos);
                }
                match Frame::decode(&self.read_buf) {
                    Ok((frame, consumed)) => {
                        self.read_buf.drain(..consumed);
                        return Ok(frame);
                    }
                    Err(crate::core::error::FrameError::Truncated { .. }) => break,
                    Err(e) => return Err(e.into()),
                }
            }

            // Need more data from port
            if std::time::Instant::now() >= deadline {
                return Err(CoreError::Frame(crate::core::error::FrameError::TooShort(
                    self.read_buf.len(),
                )));
            }

            let n = self.port.read(&mut tmp)?;
            if n == 0 {
                return Err(CoreError::Frame(crate::core::error::FrameError::TooShort(
                    self.read_buf.len(),
                )));
            }
            self.read_buf.extend_from_slice(&tmp[..n]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::*;
    use crate::core::error::CommandError;
    use crate::core::frame::FrameType;
    use crate::util::PushU16;
    use crate::Region;
    use std::io::{Cursor, Read, Write};

    struct MockStream {
        read_data: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl MockStream {
        fn with_response(data: &[u8]) -> Self {
            Self {
                read_data: Cursor::new(data.to_vec()),
                written: Vec::new(),
            }
        }
    }

    impl Read for MockStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.read_data.read(buf)
        }
    }

    impl Write for MockStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn response_frame(command_code: u8, data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.push(FRAME_HEADER);
        buf.push(FrameType::Response as u8);
        buf.push(command_code);
        buf.push_u16(data.len() as u16);
        buf.extend_from_slice(data);
        let cs: u8 = buf[1..].iter().fold(0u16, |a, &b| a + b as u16) as u8;
        buf.push(cs);
        buf.push(0xDD);
        buf
    }

    #[test]
    fn get_module_info() {
        let resp = response_frame(0x03, b"V2.3.5");
        let mut reader = SyncReader::new(MockStream::with_response(&resp));
        let info = reader
            .send(&GetModuleInfo {
                param: ModuleInfoParam::SoftwareVersion,
            })
            .unwrap();
        assert_eq!(info.text, "V2.3.5");
    }

    #[test]
    fn single_polling_no_tag() {
        let resp = response_frame(0x22, &[]);
        let mut reader = SyncReader::new(MockStream::with_response(&resp));
        let tag = reader.send(&SinglePollingInstruction).unwrap();
        assert!(tag.is_none());
    }

    #[test]
    fn single_polling_with_tag() {
        let tag_data = vec![
            0xAB, 0x30, 0x00, 0xE2, 0x00, 0x10, 0x11, 0x22, 0x33, 0x44, 0x55,
            0x66, 0x77, 0x88, 0x99, 0x00, 0x00,
        ];
        let resp = response_frame(0x22, &tag_data);
        let mut reader = SyncReader::new(MockStream::with_response(&resp));
        let tag = reader.send(&SinglePollingInstruction).unwrap().unwrap();
        assert_eq!(tag.rssi, 0xAB);
    }

    #[test]
    fn get_transmit_power() {
        let resp = response_frame(0xB7, &[0x09, 0x9C]);
        let mut reader = SyncReader::new(MockStream::with_response(&resp));
        let power = reader.send(&GetTransmitPower).unwrap();
        assert!((power - 24.6).abs() < 0.01);
    }

    #[test]
    fn get_working_area() {
        let resp = response_frame(0x08, &[0x03]);
        let mut reader = SyncReader::new(MockStream::with_response(&resp));
        let region = reader.send(&GetWorkingArea).unwrap();
        assert_eq!(region, Region::Eu);
    }

    #[test]
    fn command_error_response() {
        let resp = response_frame(0xFF, &[0x09]);
        let mut reader = SyncReader::new(MockStream::with_response(&resp));
        let err = reader.send(&SinglePollingInstruction).unwrap_err();
        assert!(matches!(err, CoreError::Command(CommandError(0x09))));
    }

    #[test]
    fn multiple_frames_in_stream() {
        let r1 = response_frame(0x03, b"V2.3.5");
        let r2 = response_frame(0xB7, &[0x09, 0x9C]);
        let mut combined = r1;
        combined.extend_from_slice(&r2);

        let mut reader = SyncReader::new(MockStream::with_response(&combined));
        let info = reader
            .send(&GetModuleInfo {
                param: ModuleInfoParam::SoftwareVersion,
            })
            .unwrap();
        assert_eq!(info.text, "V2.3.5");

        let power = reader.send(&GetTransmitPower).unwrap();
        assert!((power - 24.6).abs() < 0.01);
    }

    #[test]
    fn noise_before_frame() {
        let noise = vec![0x00, 0xFF, 0x42, 0x13];
        let resp = response_frame(0x08, &[0x02]);
        let mut combined = noise;
        combined.extend_from_slice(&resp);

        let mut reader = SyncReader::new(MockStream::with_response(&combined));
        let region = reader.send(&GetWorkingArea).unwrap();
        assert_eq!(region, Region::Us);
    }
}

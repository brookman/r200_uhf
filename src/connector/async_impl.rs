use crate::connector::{
    Connector, ConnectorError, WorkingArea, calculate_transmit_power, clear_non_ascii, hex_lower,
    hexdump_line, parse_hex_str, strip_read_framing,
};
use crate::frame::{Command, ErrorCode, Frame, R200_FRAME_HEADER};
use crate::packet::Packet;
use crate::rfid::Rfid;
use async_trait::async_trait;
use log::{debug, error};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[async_trait]
/// Async (tokio) API for talking to the R200 reader. Same operations as
/// [`SyncIO`](crate::connector::sync::SyncIO) but non-blocking.
pub trait AsyncIO {
    type Socket: AsyncRead + AsyncWrite + Unpin + Send;
    /// Setup the reader with default settings (inspired by e710_uhf).
    async fn setup_reader(&mut self) -> Result<(), ConnectorError>;
    /// Query the reader module info (hardware, software, manufacturer).
    async fn get_module_info(&mut self) -> Result<String, ConnectorError>;
    /// Builds and sends the command.
    async fn send_packet(&mut self, command: Command) -> Result<(), ConnectorError>;
    /// Read a single response packet from the serial port.
    async fn single_read_from_serial(&mut self) -> Result<Option<Packet>, ConnectorError>;
    /// Read multiple response packets (used by multi-polling).
    async fn read_from_serial(
        &mut self,
        num_expected_responses: Option<u32>,
    ) -> Result<Option<Vec<Packet>>, ConnectorError>;
    /// Get the current regulatory working area configured on the device.
    async fn get_working_area(&mut self) -> Result<WorkingArea, ConnectorError>;
    /// Get the current working RF channel as a frequency in MHz.
    async fn get_working_channel(&mut self) -> Result<f64, ConnectorError>;
    /// Read the current transmit power reported by the device (typically dBm).
    async fn get_transmit_power(&mut self) -> Result<f64, ConnectorError>;
    /// Set the transmitter output power (typically dBm).
    async fn set_transmission_power(&mut self, power: f64) -> Result<(), ConnectorError>;
    /// Perform a single inventory (poll) and return the detected tags.
    async fn single_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError>;
    /// Run a multi-polling inventory round and collect all detected tags.
    async fn multi_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError>;
    /// Stop the reader's multi-polling mode.
    async fn stop_multiple_polling_instructions(&mut self) -> Result<(), ConnectorError>;
    /// Set the regulatory working area on the device.
    async fn set_working_area(&mut self, area: WorkingArea) -> Result<(), ConnectorError>;
    /// Select a tag by EPC for subsequent operations.
    async fn select_tag(&mut self, epc: &[u8]) -> Result<(), ConnectorError>;
    /// Clear the current tag selection.
    async fn clear_select(&mut self) -> Result<(), ConnectorError>;
    /// Read data from a selected tag's memory bank.
    ///
    /// `access_password` is 4 bytes (use `00000000` unless a non-default
    /// access password is set). `mem_bank` is the Gen2 memory bank:
    /// 0=Reserved, 1=EPC, 2=TID, 3=User. `start_addr` is the starting word
    /// address and `length` is the number of words to read.
    ///
    /// Returns only the requested words; the RSSI/PC/EPC framing of the
    /// reader response is stripped.
    async fn read_mem(
        &mut self,
        access_password: &[u8],
        mem_bank: u8,
        start_addr: u16,
        length: u16,
    ) -> Result<Vec<u8>, ConnectorError>;
    /// Read the EPC of the selected tag (convenience over [`AsyncIO::read_mem`]).
    async fn read_epc(&mut self) -> Result<Vec<u8>, ConnectorError> {
        self.read_mem(&[0x00, 0x00, 0x00, 0x00], 0x01, 0x0002, 6)
            .await
    }
    /// Write a new EPC to a tag.
    async fn write_epc(&mut self, epc: &[u8]) -> Result<(), ConnectorError>;
    /// Write data to a memory bank of the selected tag.
    ///
    /// The tag must be selected first (see [`AsyncIO::select_tag`]). `data`
    /// length must be a multiple of 2 (whole words). `mem_bank` is the Gen2
    /// memory bank: 0=Reserved, 1=EPC, 2=TID, 3=User.
    async fn write_mem(
        &mut self,
        access_password: &[u8],
        mem_bank: u8,
        start_addr: u16,
        data: &[u8],
    ) -> Result<(), ConnectorError>;
    /// Kill a previously selected tag.
    ///
    /// The tag must be selected first (see [`AsyncIO::select_tag`]). The kill
    /// password is 4 bytes. A tag with the default (all-zero) kill password
    /// cannot be killed — write a non-zero kill password to reserved memory
    /// (bank 0, word 0) first, then pass it here.
    async fn kill_tag(&mut self, kill_password: &[u8]) -> Result<(), ConnectorError>;
    /// Lock a previously selected tag.
    ///
    /// The tag must be selected first (see [`AsyncIO::select_tag`]). The access
    /// password is 4 bytes and the lock operation is 3 bytes (20-bit
    /// mask/action payload, e.g. `02 00 80` to lock the USER memory bank).
    async fn lock_tag(
        &mut self,
        access_password: &[u8],
        lock_data: &[u8],
    ) -> Result<(), ConnectorError>;
    /// Write EPC with automatic select + retry.
    async fn write_epc_reliable(
        &mut self,
        current_epc: &[u8],
        new_epc: &[u8],
        max_retries: usize,
    ) -> Result<(), ConnectorError>;
}

#[async_trait]
impl<S> AsyncIO for Connector<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    type Socket = S;

    async fn setup_reader(&mut self) -> Result<(), ConnectorError> {
        self.stop_multiple_polling_instructions().await.ok();
        Ok(())
    }

    async fn get_module_info(&mut self) -> Result<String, ConnectorError> {
        self.send_packet(Command::HardwareVersion).await?;
        let hardware = self.single_read_from_serial().await?;
        self.send_packet(Command::SoftwareVersion).await?;
        let software = self.single_read_from_serial().await?;
        self.send_packet(Command::Manufacturer).await?;
        let manufacture = self.single_read_from_serial().await?;

        let hw_str = hardware.map(|p| p.to_string()).unwrap_or_default();
        let sw_str = software.map(|p| p.to_string()).unwrap_or_default();
        let mf_str = manufacture.map(|p| p.to_string()).unwrap_or_default();

        let out = format!(
            "Hardware: {} - Software: {} - Manufacturer: {}",
            clear_non_ascii(&hw_str),
            clear_non_ascii(&sw_str),
            clear_non_ascii(&mf_str)
        );

        Ok(out)
    }

    async fn send_packet(&mut self, command: Command) -> Result<(), ConnectorError> {
        let frame = Frame::new(&command).to_bytes();

        let mut out = String::new();
        for b in &frame {
            out.push_str(format!("{:02X} ", b).as_str());
        }
        debug!("[TX] {out} - [{command}]");

        self.port.write_all(&frame).await?;
        self.port.flush().await?;
        Ok(())
    }

    async fn single_read_from_serial(&mut self) -> Result<Option<Packet>, ConnectorError> {
        let out = self.read_from_serial(Some(1)).await?;
        Ok(out.unwrap_or(vec![]).pop())
    }

    async fn read_from_serial(
        &mut self,
        num_expected_responses: Option<u32>,
    ) -> Result<Option<Vec<Packet>>, ConnectorError> {
        let mut read_buf: [u8; 1024] = [0u8; 1024];
        let mut rolling: Vec<u8> = Vec::with_capacity(4096);
        let mut output: Vec<Packet> = Vec::new();

        loop {
            let read_future = self.port.read(&mut read_buf);

            // In a real async scenario with timeout, we might use tokio::time::timeout
            let raw_data_size =
                match tokio::time::timeout(Duration::from_millis(500), read_future).await {
                    Ok(res) => res,
                    Err(_) => {
                        if output.is_empty() {
                            return Err(ConnectorError::Timeout);
                        }
                        break;
                    }
                };

            match raw_data_size {
                Ok(n) if n > 0 => {
                    rolling.extend_from_slice(&read_buf[..n]);
                    hexdump_line("[RAW] ", &rolling);

                    // Frame format: AA TYPE CMD PL_MSB PL_LSB DATA[PL] CHECKSUM DD.
                    // The frame end is computed from the PL length field, not by
                    // scanning for 0xDD: 0xDD may legitimately appear inside the
                    // data or the checksum of a valid frame.
                    let mut consumed = 0;
                    while consumed < rolling.len() {
                        let Some(header_rel) = rolling[consumed..]
                            .iter()
                            .position(|&x| x == R200_FRAME_HEADER)
                        else {
                            rolling.clear();
                            consumed = 0;
                            break;
                        };
                        consumed += header_rel;
                        // Need header (3) + length (2) before the frame length is known.
                        if consumed + 5 > rolling.len() {
                            rolling.drain(..consumed);
                            consumed = 0;
                            break;
                        }
                        let data_len = ((rolling[consumed + 3] as usize) << 8)
                            | rolling[consumed + 4] as usize;
                        let frame_len = 5 + data_len + 2; // header + len + data + checksum + end
                        if consumed + frame_len > rolling.len() {
                            // Incomplete frame: keep the remainder aligned to the header.
                            rolling.drain(..consumed);
                            consumed = 0;
                            break;
                        }
                        let chunk = &rolling[consumed..consumed + frame_len];
                        let p = Packet::new(Vec::from(chunk));

                        if p.is_valid() {
                            debug!("{}", p.debug());
                            output.push(p);
                            if output.len() >= num_expected_responses.unwrap_or(100000) as usize {
                                return Ok(Some(output));
                            }
                        } else {
                            error!("Invalid packet: {:?}", chunk);
                        }
                        consumed += frame_len;
                    }

                    if consumed > 0 {
                        rolling.drain(..consumed);
                    }

                    if rolling.len() > 8192 {
                        rolling.drain(..rolling.len() - 4096);
                    }
                }
                Ok(_) => return Ok(None),
                Err(ref e) if e.kind() == io::ErrorKind::TimedOut => {
                    // Serial timeout: stop collecting and return what we have,
                    // matching the sync implementation.
                    if output.is_empty() {
                        return Err(ConnectorError::Timeout);
                    }
                    break;
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ConnectorError::SerialRead(e.to_string())),
            }
        }
        Ok(Some(output))
    }

    async fn get_working_area(&mut self) -> Result<WorkingArea, ConnectorError> {
        self.send_packet(Command::GetWorkingArea).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            return Connector::<S>::parse_to_working_area(p);
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn get_working_channel(&mut self) -> Result<f64, ConnectorError> {
        self.send_packet(Command::GetWorkingChannel).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            return Ok(self.get_working_area().await?.packet_to_64(p));
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn get_transmit_power(&mut self) -> Result<f64, ConnectorError> {
        self.send_packet(Command::AcquireTransmitPower).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            return calculate_transmit_power(p);
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn set_transmission_power(&mut self, power: f64) -> Result<(), ConnectorError> {
        self.send_packet(Command::SetTransmissionPower(power))
            .await?;
        Connector::<S>::_set_transmission_power(self.single_read_from_serial().await?, power)
    }

    async fn single_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError> {
        self.send_packet(Command::SinglePollingInstruction).await?;
        let response = self.read_from_serial(None).await?;
        self.parse_rfid_packets(response)
    }

    async fn multi_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError> {
        self.send_packet(Command::MultiplePollingInstruction(100))
            .await?;
        let response = self.read_from_serial(Some(100)).await?;
        self.parse_rfid_packets(response)
    }

    async fn stop_multiple_polling_instructions(&mut self) -> Result<(), ConnectorError> {
        self.send_packet(Command::StopMultiplePollingInstruction)
            .await?;
        // In-flight tag notifications may still arrive alongside the 0x28
        // acknowledgement; keep reading until the acknowledgement is seen.
        match self.read_from_serial(None).await {
            Ok(Some(packets))
                if packets.iter().any(|p| {
                    matches!(p.command(), Ok(Command::StopMultiplePollingInstruction))
                }) =>
            {
                Ok(())
            }
            _ => Err(ConnectorError::ErrorStopMultiPolling(
                "No stop acknowledgement from device".into(),
            )),
        }
    }

    async fn set_working_area(&mut self, area: WorkingArea) -> Result<(), ConnectorError> {
        let code: u8 = match area {
            WorkingArea::China900Mhz => 1,
            WorkingArea::US => 2,
            WorkingArea::EU => 3,
            WorkingArea::China800Mhz => 4,
            WorkingArea::Korea => 6,
        };
        self.send_packet(Command::SetWorkingArea(code)).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn select_tag(&mut self, epc: &[u8]) -> Result<(), ConnectorError> {
        let mut params = Vec::new();
        // SelParam 0x01 (target 3'b000, action 3'b000, MemBank 2'b01 = EPC),
        // Ptr 0x00000020 (bit pointer, not word — EPC bank start), MaskLen
        // 0x60 (6 words = 96 bits), Truncate 0x00 (disabled), then the EPC mask.
        params.push(0x01);
        params.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]);
        params.push(0x60);
        params.push(0x00);
        params.extend_from_slice(epc);
        self.send_packet(Command::SetSelect(params)).await?;
        check_ack(self.single_read_from_serial().await?)?;
        // Select mode 0x02 = send the Select command before every tag operation
        // other than polling inventory (read, write, lock, kill). Without it the
        // module stores the mask but never applies it to tag operations.
        self.send_packet(Command::SetSendSelect(0x02)).await?;
        check_ack(self.single_read_from_serial().await?)?;
        Ok(())
    }

    async fn clear_select(&mut self) -> Result<(), ConnectorError> {
        let params = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        self.send_packet(Command::SetSelect(params)).await?;
        check_ack(self.single_read_from_serial().await?)?;
        // Select mode 0x01 = do not send the Select command before tag operations.
        self.send_packet(Command::SetSendSelect(0x01)).await?;
        check_ack(self.single_read_from_serial().await?)?;
        Ok(())
    }

    async fn read_mem(
        &mut self,
        access_password: &[u8],
        mem_bank: u8,
        start_addr: u16,
        length: u16,
    ) -> Result<Vec<u8>, ConnectorError> {
        let mut params = Vec::new();
        params.extend_from_slice(access_password);
        params.push(mem_bank);
        params.push((start_addr >> 8) as u8);
        params.push((start_addr & 0xFF) as u8);
        params.push((length >> 8) as u8);
        params.push((length & 0xFF) as u8);
        self.send_packet(Command::ReadLabel(params)).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return strip_read_framing(&p.get_data());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn write_epc(&mut self, epc: &[u8]) -> Result<(), ConnectorError> {
        self.write_mem(&[0x00, 0x00, 0x00, 0x00], 0x01, 0x0002, epc)
            .await
    }

    async fn write_mem(
        &mut self,
        access_password: &[u8],
        mem_bank: u8,
        start_addr: u16,
        data: &[u8],
    ) -> Result<(), ConnectorError> {
        if !data.len().is_multiple_of(2) {
            return Err(ConnectorError::FailedSetting(
                "write data must be an even number of bytes".to_string(),
            ));
        }
        let word_len = (data.len() / 2) as u16;
        let mut params = Vec::new();
        params.extend_from_slice(access_password);
        params.push(mem_bank);
        params.push((start_addr >> 8) as u8);
        params.push((start_addr & 0xFF) as u8);
        params.push((word_len >> 8) as u8);
        params.push((word_len & 0xFF) as u8);
        params.extend_from_slice(data);
        self.send_packet(Command::WriteLabel(params)).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn kill_tag(&mut self, kill_password: &[u8]) -> Result<(), ConnectorError> {
        let params = kill_password.to_vec();
        self.send_packet(Command::KillTag(params)).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn lock_tag(
        &mut self,
        access_password: &[u8],
        lock_data: &[u8],
    ) -> Result<(), ConnectorError> {
        let mut params = Vec::with_capacity(7);
        params.extend_from_slice(access_password);
        params.extend_from_slice(lock_data);
        self.send_packet(Command::LockTag(params)).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn write_epc_reliable(
        &mut self,
        current_epc: &[u8],
        new_epc: &[u8],
        max_retries: usize,
    ) -> Result<(), ConnectorError> {
        let mut last_err = None;
        let mut select_epc = current_epc.to_vec();

        for attempt in 0..=max_retries {
            self.select_tag(&select_epc).await?;
            match self.write_epc(new_epc).await {
                Ok(()) => return Ok(()),
                Err(ConnectorError::CommandError(ErrorCode::WriteFail)) => {
                    debug!(
                        "[write_epc] attempt {}/{} failed: WriteFail, polling to rediscover tag",
                        attempt + 1,
                        max_retries + 1
                    );
                    last_err = Some(ConnectorError::CommandError(ErrorCode::WriteFail));
                    self.clear_select().await.ok();
                    if let Ok(tags) = self.single_polling_instruction().await
                        && let Some(tag) = tags.first()
                    {
                        let discovered = parse_hex_str(&tag.epc);
                        debug!(
                            "[write_epc] rediscovered tag EPC: {} (expected: {})",
                            tag.epc,
                            hex_lower(&select_epc)
                        );
                        select_epc = discovered;
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap())
    }
}

/// Convert a single-read result into a success/error result.
fn check_ack(p: Option<Packet>) -> Result<(), ConnectorError> {
    if let Some(p) = p {
        if p.is_error() {
            let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
            return Err(ConnectorError::CommandError(error_code));
        }
        return Ok(());
    }
    Err(ConnectorError::NoPacketReceived)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::R200_FRAME_END;
    use std::collections::VecDeque;
    use std::io;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite};

    struct MockAsyncPort {
        // Bytes available for reading. Response frames are injected here when a
        // command is written, so a polling read never consumes responses that
        // belong to commands not yet sent.
        read_data: VecDeque<u8>,
        // Response frame sets, one per written command, injected in order.
        response_sets: VecDeque<Vec<u8>>,
        written_data: Arc<Mutex<Vec<u8>>>,
    }

    impl MockAsyncPort {
        fn new(response_sets: Vec<Vec<u8>>) -> Self {
            MockAsyncPort {
                read_data: VecDeque::new(),
                response_sets: response_sets.into(),
                written_data: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl AsyncRead for MockAsyncPort {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.read_data.is_empty() {
                // Go silent until the next command is written, so the reader's
                // tokio timeout fires (as with a quiet device).
                return Poll::Pending;
            }
            // Return a single complete frame at a time.
            let n = if let Some(end) = self.read_data.iter().position(|&b| b == R200_FRAME_END) {
                (end + 1).min(buf.remaining())
            } else {
                std::cmp::min(buf.remaining(), self.read_data.len())
            };
            let data: Vec<u8> = self.read_data.drain(..n).collect();
            buf.put_slice(&data);
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for MockAsyncPort {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let mut written = self.written_data.lock().unwrap();
            written.extend_from_slice(buf);
            // Each write_all sends one complete frame; reveal the matching
            // response frames for it.
            let frame_complete = written.last() == Some(&R200_FRAME_END);
            drop(written);
            if frame_complete {
                let me = self.get_mut();
                if let Some(set) = me.response_sets.pop_front() {
                    me.read_data.extend(set);
                }
            }
            Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    // Helper: a port whose single command response is one frame.
    fn mock_port(resp: Vec<u8>) -> MockAsyncPort {
        MockAsyncPort::new(vec![resp])
    }

    // Helper: a port whose responses are grouped per written command.
    fn mock_port_sets(sets: Vec<Vec<u8>>) -> MockAsyncPort {
        MockAsyncPort::new(sets)
    }

    #[tokio::test]
    async fn test_async_get_module_info() {
        let mut connector = Connector::new(mock_port_sets(vec![
            make_rx_frame(0x03, b"HW1.0"),
            make_rx_frame(0x03, b"SW2.0"),
            make_rx_frame(0x03, b"ACME"),
        ]));
        let info = connector.get_module_info().await.unwrap();
        assert!(info.contains("Hardware: HW1.0"));
        assert!(info.contains("Software: SW2.0"));
        assert!(info.contains("Manufacturer: ACME"));
    }

    // Helper: build a valid device->PC frame with the given command code and data.
    fn make_rx_frame(cmd: u8, data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.push(R200_FRAME_HEADER);
        v.push(0x01); // Device to PC
        v.push(cmd);
        let len = data.len() as u16;
        v.push((len >> 8) as u8);
        v.push((len & 0xFF) as u8);
        v.extend_from_slice(data);
        let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
        v.push((sum & 0xFF) as u8);
        v.push(R200_FRAME_END);
        v
    }

    #[tokio::test]
    async fn test_async_kill_tag_success() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0x65, &[])));
        connector.kill_tag(&[0x00, 0x00, 0xFF, 0xFF]).await.unwrap();
    }

    #[tokio::test]
    async fn test_async_lock_tag_success() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0x82, &[])));
        connector
            .lock_tag(&[0x00, 0x00, 0xFF, 0xFF], &[0x02, 0x00, 0x80])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_async_kill_tag_error() {
        // Error packet: command 0xFF with code 0x12 (KillFail)
        let mut connector = Connector::new(mock_port(make_rx_frame(0xFF, &[0x12])));
        let err = connector
            .kill_tag(&[0x00, 0x00, 0xFF, 0xFF])
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::KillFail)
        ));
    }

    #[tokio::test]
    async fn test_async_read_mem_success() {
        let mut resp = vec![0x0E];
        resp.extend_from_slice(&[0x30, 0x00]);
        resp.extend_from_slice(&[0u8; 12]);
        resp.extend_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        let mut connector = Connector::new(mock_port(make_rx_frame(0x39, &resp)));
        let data = connector
            .read_mem(&[0x12, 0x34, 0x56, 0x78], 0, 0, 2)
            .await
            .unwrap();
        assert_eq!(data, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[tokio::test]
    async fn test_async_read_mem_error() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0xFF, &[0x10])));
        let err = connector
            .read_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, 2)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::WriteFail)
        ));
    }

    #[tokio::test]
    async fn test_async_write_mem_success() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0x49, &[])));
        connector
            .write_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, &[0x00, 0x00, 0x00, 0x00])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_async_write_mem_error() {
        // Error packet: command 0xFF with code 0x10 (WriteFail)
        let mut connector = Connector::new(mock_port(make_rx_frame(0xFF, &[0x10])));
        let err = connector
            .write_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, &[0x00, 0x00, 0x00, 0x00])
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::WriteFail)
        ));
    }

    // Helper: build a 17-byte tag response frame payload.
    fn tag_payload(epc: &[u8]) -> Vec<u8> {
        assert_eq!(epc.len(), 12);
        let mut data = vec![55, 0x30, 0x00];
        data.extend_from_slice(epc);
        data.extend_from_slice(&[0xAB, 0xCD]);
        assert_eq!(data.len(), 17);
        data
    }

    #[tokio::test]
    async fn test_async_setup_reader() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0x28, &[])));
        connector.setup_reader().await.unwrap();
    }

    #[tokio::test]
    async fn test_async_get_working_area() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0x08, &[3])));
        let area = connector.get_working_area().await.unwrap();
        assert_eq!(format!("{:?}", area), "EU");
    }

    #[tokio::test]
    async fn test_async_get_working_channel() {
        // Channel index 4 + EU area: 4 * 0.2 + 865.1 MHz
        let mut connector = Connector::new(mock_port_sets(vec![
            make_rx_frame(0xAA, &[4]),
            make_rx_frame(0x08, &[3]),
        ]));
        let freq = connector.get_working_channel().await.unwrap();
        assert!((freq - (4.0 * 0.2 + 865.1)).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_async_get_transmit_power() {
        // 0x0A, 0xBE => 2750 => 27.50
        let mut connector = Connector::new(mock_port(make_rx_frame(0xB7, &[0x0A, 0xBE])));
        let p = connector.get_transmit_power().await.unwrap();
        assert!((p - 27.50).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_async_set_transmission_power_ack() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0xB6, &[0x00])));
        connector.set_transmission_power(20.0).await.unwrap();
    }

    #[tokio::test]
    async fn test_async_set_working_area_success() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0x07, &[])));
        connector.set_working_area(WorkingArea::EU).await.unwrap();
    }

    #[tokio::test]
    async fn test_async_set_working_area_error() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0xFF, &[0x16])));
        let err = connector
            .set_working_area(WorkingArea::EU)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::AccessFail)
        ));
    }

    #[tokio::test]
    async fn test_async_select_tag_success() {
        let epc = [
            0xE0, 0x28, 0x06, 0x91, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut connector = Connector::new(mock_port_sets(vec![
            make_rx_frame(0x0C, &[]),  // mask ack
            make_rx_frame(0x12, &[]),  // send-select ack
        ]));
        connector.select_tag(&epc).await.unwrap();
    }

    #[tokio::test]
    async fn test_async_clear_select_success() {
        let mut connector = Connector::new(mock_port_sets(vec![
            make_rx_frame(0x0C, &[]),  // mask ack
            make_rx_frame(0x12, &[]),  // send-select ack
        ]));
        connector.clear_select().await.unwrap();
    }

    #[tokio::test]
    async fn test_async_stop_multiple_polling_success() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0x28, &[])));
        connector
            .stop_multiple_polling_instructions()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_async_stop_multiple_polling_wrong_response() {
        let mut connector = Connector::new(mock_port(make_rx_frame(0xFF, &[0x12])));
        let err = connector
            .stop_multiple_polling_instructions()
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectorError::ErrorStopMultiPolling(_)));
    }

    #[tokio::test]
    async fn test_async_read_epc() {
        // read_epc() = read_mem(0000, bank 1, addr 2, 6 words).
        // Response: ul=14, PC(2), EPC(12), then 12 requested bytes (no 0xDD inside).
        let mut resp = vec![0x0E];
        resp.extend_from_slice(&[0x30, 0x00]);
        resp.extend_from_slice(&[0u8; 12]);
        resp.extend_from_slice(&[
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x01, 0x02, 0x03,
        ]);
        let mut connector = Connector::new(mock_port(make_rx_frame(0x39, &resp)));
        let data = connector.read_epc().await.unwrap();
        assert_eq!(
            data,
            vec![
                0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x01, 0x02, 0x03
            ]
        );
    }

    #[tokio::test]
    async fn test_async_write_epc_success() {
        let epc = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x00];
        let mut connector = Connector::new(mock_port(make_rx_frame(0x49, &[])));
        connector.write_epc(&epc).await.unwrap();
    }

    #[tokio::test]
    async fn test_async_write_epc_reliable_success_first_try() {
        let current = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x00];
        let new = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x01];
        let mut connector = Connector::new(mock_port_sets(vec![
            make_rx_frame(0x0C, &[]), // select mask ack
            make_rx_frame(0x12, &[]), // select mode ack
            make_rx_frame(0x49, &[]), // write ack
        ]));
        connector
            .write_epc_reliable(&current, &new, 1)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_async_write_epc_reliable_retries_on_write_fail() {
        let current = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x00];
        let new = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x01];
        let mut connector = Connector::new(mock_port_sets(vec![
            make_rx_frame(0x0C, &[]),     // select mask ack (attempt 0)
            make_rx_frame(0x12, &[]),     // select mode ack (attempt 0)
            make_rx_frame(0xFF, &[0x10]), // write -> WriteFail
            make_rx_frame(0x0C, &[]),     // clear select mask ack
            make_rx_frame(0x12, &[]),     // clear select mode ack
            make_rx_frame(
                0x22,
                &tag_payload(&[
                    0xE0, 0x28, 0x06, 0x91, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                ]),
            ), // poll
            make_rx_frame(0x0C, &[]),     // select mask ack (attempt 1)
            make_rx_frame(0x12, &[]),     // select mode ack (attempt 1)
            make_rx_frame(0x49, &[]),     // write ack (attempt 1)
        ]));
        connector
            .write_epc_reliable(&current, &new, 1)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_async_multi_polling_instruction_parses_tags() {
        let frames: Vec<u8> = (0..100u8)
            .flat_map(|i| {
                let mut epc = [0u8; 12];
                epc[0] = 0xE0;
                epc[5] = i;
                make_rx_frame(0x27, &tag_payload(&epc))
            })
            .collect();
        let mut connector = Connector::new(mock_port_sets(vec![frames]));
        let tags = connector.multi_polling_instruction().await.unwrap();
        assert_eq!(tags.len(), 100);
        let expected: String = {
            let mut epc = [0u8; 12];
            epc[0] = 0xE0;
            epc[5] = 0x00;
            epc.iter().map(|b| format!("{:02X}", b)).collect()
        };
        assert_eq!(tags[0].uid(), expected);
    }
}

use crate::connector::{
    Connector, ConnectorError, WorkingArea, calculate_transmit_power, clear_non_ascii, hex_lower,
    hexdump_line, parse_hex_str,
};
use crate::frame::{Command, ErrorCode, Frame, R200_FRAME_END, R200_FRAME_HEADER};
use crate::packet::Packet;
use crate::rfid::Rfid;
use async_trait::async_trait;
use log::debug;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[async_trait]
pub trait AsyncIO {
    type Socket: AsyncRead + AsyncWrite + Unpin + Send;
    async fn setup_reader(&mut self) -> Result<(), ConnectorError>;
    async fn get_module_info(&mut self) -> Result<String, ConnectorError>;
    async fn send_packet(&mut self, command: Command) -> Result<(), ConnectorError>;
    async fn single_read_from_serial(&mut self) -> Result<Option<Packet>, ConnectorError>;
    async fn read_from_serial(
        &mut self,
        num_expected_responses: Option<u32>,
    ) -> Result<Option<Vec<Packet>>, ConnectorError>;
    async fn get_working_area(&mut self) -> Result<WorkingArea, ConnectorError>;
    async fn get_working_channel(&mut self) -> Result<f64, ConnectorError>;
    async fn get_transmit_power(&mut self) -> Result<f64, ConnectorError>;
    async fn set_transmission_power(&mut self, power: f64) -> Result<(), ConnectorError>;
    async fn single_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError>;
    async fn multi_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError>;
    async fn stop_multiple_polling_instructions(&mut self) -> Result<(), ConnectorError>;
    async fn set_working_area(&mut self, area: WorkingArea) -> Result<(), ConnectorError>;
    async fn select_tag(&mut self, epc: &[u8]) -> Result<(), ConnectorError>;
    async fn clear_select(&mut self) -> Result<(), ConnectorError>;
    async fn read_epc(
        &mut self,
        mem_bank: u8,
        start_addr: u16,
        length: u16,
    ) -> Result<Vec<u8>, ConnectorError>;
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
    /// password is 4 bytes; a tag shipped with the default password of
    /// `00000000` can be killed by any reader.
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

                    while let Some(header_pos) =
                        rolling.iter().position(|&x| x == R200_FRAME_HEADER)
                    {
                        if let Some(end_pos) = rolling.iter().position(|&x| x == R200_FRAME_END) {
                            if end_pos > header_pos {
                                let chunk = &rolling[header_pos..=end_pos];
                                if chunk.len() > 4 {
                                    let p = Packet::new(Vec::from(chunk));
                                    if p.is_valid() {
                                        debug!("{}", p.debug());
                                        output.push(p);
                                        if output.len()
                                            >= num_expected_responses.unwrap_or(100000) as usize
                                        {
                                            return Ok(Some(output));
                                        }
                                    }
                                }
                                rolling.drain(..=end_pos);
                            } else {
                                // End before header, discard everything before header
                                rolling.drain(..header_pos);
                                break;
                            }
                        } else {
                            // Header but no end yet
                            break;
                        }
                    }

                    if rolling.len() > 8192 {
                        rolling.drain(..rolling.len() - 4096);
                    }
                }
                Ok(_) => return Ok(None),
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
        if let Some(p) = self.single_read_from_serial().await? {
            if matches!(p.command(), Ok(Command::StopMultiplePollingInstruction)) {
                return Ok(());
            }
        }
        Err(ConnectorError::ErrorStopMultiPolling(
            "Failed to stop multi polling".into(),
        ))
    }

    async fn set_working_area(&mut self, area: WorkingArea) -> Result<(), ConnectorError> {
        let code: u8 = match area {
            WorkingArea::China900Mhz => 0,
            WorkingArea::China800Mhz => 1,
            WorkingArea::US => 2,
            WorkingArea::EU => 3,
            WorkingArea::Korea => 4,
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
        params.push(0x01);
        params.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]);
        params.push(0x60);
        params.push(0x00);
        params.extend_from_slice(epc);
        self.send_packet(Command::SetSelect(params)).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn clear_select(&mut self) -> Result<(), ConnectorError> {
        let params = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        self.send_packet(Command::SetSelect(params)).await?;
        if let Some(p) = self.single_read_from_serial().await? {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    async fn read_epc(
        &mut self,
        mem_bank: u8,
        start_addr: u16,
        length: u16,
    ) -> Result<Vec<u8>, ConnectorError> {
        let mut params = Vec::new();
        params.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
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
            return Ok(p.get_data());
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
        if data.len() % 2 != 0 {
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
                    if let Ok(tags) = self.single_polling_instruction().await {
                        if let Some(tag) = tags.first() {
                            let discovered = parse_hex_str(&tag.epc);
                            debug!(
                                "[write_epc] rediscovered tag EPC: {} (expected: {})",
                                tag.epc,
                                hex_lower(&select_epc)
                            );
                            select_epc = discovered;
                        }
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite};

    struct MockAsyncPort {
        read_data: Vec<u8>,
        written_data: Arc<Mutex<Vec<u8>>>,
    }

    impl AsyncRead for MockAsyncPort {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.read_data.is_empty() {
                // Return EOF if empty to avoid infinite loop or timeout in tests
                return Poll::Ready(Ok(()));
            }
            let n = std::cmp::min(buf.remaining(), self.read_data.len());
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
            self.written_data.lock().unwrap().extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn test_async_get_module_info() {
        // Mock response for Hardware, Software, Manufacturer
        // For simplicity, just one valid packet
        let mut resp = Vec::new();
        // Hardware Version (Command 0x03)
        let mut f1 = Frame::new(&Command::HardwareVersion).to_bytes();
        // Replace TX frame with RX frame for test (mocking device response)
        f1[1] = 0x01; // Device to PC
        resp.extend_from_slice(&f1);

        // Software Version
        let mut f2 = Frame::new(&Command::SoftwareVersion).to_bytes();
        f2[1] = 0x01;
        resp.extend_from_slice(&f2);

        // Manufacturer
        let mut f3 = Frame::new(&Command::Manufacturer).to_bytes();
        f3[1] = 0x01;
        resp.extend_from_slice(&f3);

        let port = MockAsyncPort {
            read_data: resp,
            written_data: Arc::new(Mutex::new(Vec::new())),
        };
        let mut connector = Connector::new(port);
        let info = connector.get_module_info().await.unwrap();
        assert!(info.contains("Hardware"));
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
        let resp = make_rx_frame(0x65, &[]);
        let port = MockAsyncPort {
            read_data: resp,
            written_data: Arc::new(Mutex::new(Vec::new())),
        };
        let mut connector = Connector::new(port);
        connector.kill_tag(&[0x00, 0x00, 0xFF, 0xFF]).await.unwrap();
    }

    #[tokio::test]
    async fn test_async_lock_tag_success() {
        let resp = make_rx_frame(0x82, &[]);
        let port = MockAsyncPort {
            read_data: resp,
            written_data: Arc::new(Mutex::new(Vec::new())),
        };
        let mut connector = Connector::new(port);
        connector
            .lock_tag(&[0x00, 0x00, 0xFF, 0xFF], &[0x02, 0x00, 0x80])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_async_kill_tag_error() {
        // Error packet: command 0xFF with code 0x12 (KillFail)
        let resp = make_rx_frame(0xFF, &[0x12]);
        let port = MockAsyncPort {
            read_data: resp,
            written_data: Arc::new(Mutex::new(Vec::new())),
        };
        let mut connector = Connector::new(port);
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
    async fn test_async_write_mem_success() {
        let resp = make_rx_frame(0x49, &[]);
        let port = MockAsyncPort {
            read_data: resp,
            written_data: Arc::new(Mutex::new(Vec::new())),
        };
        let mut connector = Connector::new(port);
        connector
            .write_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, &[0x00, 0x00, 0x00, 0x00])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_async_write_mem_error() {
        // Error packet: command 0xFF with code 0x10 (WriteFail)
        let resp = make_rx_frame(0xFF, &[0x10]);
        let port = MockAsyncPort {
            read_data: resp,
            written_data: Arc::new(Mutex::new(Vec::new())),
        };
        let mut connector = Connector::new(port);
        let err = connector
            .write_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, &[0x00, 0x00, 0x00, 0x00])
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::WriteFail)
        ));
    }
}

use crate::connector::{
    Connector, ConnectorError, WorkingArea, calculate_transmit_power, clear_non_ascii, hex_lower,
    hexdump_line, parse_hex_str, strip_read_framing,
};
use crate::frame::{Command, ErrorCode, Frame, R200_FRAME_HEADER};
use crate::packet::Packet;
use crate::rfid::Rfid;
use log::{debug, error};
use std::io::{self, Read, Write};

/// Blocking (synchronous) API for talking to the R200 reader.
pub trait SyncIO {
    type Socket: Read + Write;
    /// Setup the reader with default settings (inspired by e710_uhf)
    fn setup_reader(&mut self) -> Result<(), ConnectorError>;
    /// Query the reader module info (hardware, software, manufacturer).
    fn get_module_info(&mut self) -> Result<String, ConnectorError>;
    /// Builds and sends the command
    fn send_packet(&mut self, command: Command) -> Result<(), ConnectorError>;
    /// Read a single response packet from the serial port.
    fn single_read_from_serial(&mut self) -> Result<Option<Packet>, ConnectorError>;
    /// Read multiple response packets (used by multi-polling).
    fn read_from_serial(
        &mut self,
        num_expected_responses: Option<u32>,
    ) -> Result<Option<Vec<Packet>>, ConnectorError>;
    /// Get the current regulatory working area configured on the device.
    ///
    /// Returns
    /// - Ok(WorkingArea) with the region inferred from the device response.
    /// - Err(ConnectorError::InvalidWorkingArea) if the response contains an unknown code.
    /// - Err(ConnectorError::NoPacketReceived) if nothing is received.
    /// - Other ConnectorError variants on I/O failure or timeout.
    fn get_working_area(&mut self) -> Result<WorkingArea, ConnectorError>;
    /// Get the current working RF channel as a frequency in MHz.
    ///
    /// The raw channel index returned by the device is converted to MHz based on
    /// the configured WorkingArea. Different regions use different spacing and base frequencies.
    ///
    /// Returns
    /// - Ok(f64) with the center frequency in MHz.
    /// - Err(ConnectorError::NoPacketReceived) if no response is obtained.
    /// - Other ConnectorError variants on I/O failure, timeout, or unknown working area.
    fn get_working_channel(&mut self) -> Result<f64, ConnectorError>;
    /// Read the current transmit power reported by the device.
    ///
    /// The device returns two bytes that represent the power value scaled by 100.
    /// This method combines them and returns the value as f64.
    ///
    /// Returns
    /// - Ok(f64) with the transmit power (device-specific units, typically dBm).
    /// - Err(ConnectorError::NoPacketReceived) if no response is obtained.
    /// - Other ConnectorError variants on I/O failure or timeout.
    fn get_transmit_power(&mut self) -> Result<f64, ConnectorError>;
    /// Set the transmitter output power.
    ///
    /// Parameters
    /// - power: Desired transmit power in device-specific units (typically dBm).
    ///
    /// Returns
    /// - Ok(()) when the device acknowledges the setting.
    /// - Err(ConnectorError::NoPacketReceived) if no response is obtained.
    /// - Other ConnectorError variants on I/O failure or timeout.
    fn set_transmission_power(&mut self, power: f64) -> Result<(), ConnectorError>;
    /// Perform a single inventory (poll) and return the list of detected tags.
    ///
    /// Sends a SinglePollingInstruction to the reader and parses all returned packets
    /// into a collection of Rfid records containing RSSI, PC, EPC (UID) and CRC.
    ///
    /// Returns
    /// - Ok(`Vec<Rfid>`) possibly empty if no tags are present.
    /// - Err(ConnectorError::Timeout or other) on communication errors.
    fn single_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError>;
    /// Run a multi-polling inventory round and collect all detected tags.
    fn multi_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError>; // Start Multi: AA 00 27 00 03 22 FF FF 4A DD
    /// Enable the reader's repeated multi-polling mode.
    fn enable_multiple_polling_instructions(
        &mut self,
        pool_times: u16,
    ) -> Result<(), ConnectorError>; // Stop Multi: AA 00 28 00 00 28 DD
    /// Stop the reader's multi-polling mode.
    fn stop_multiple_polling_instructions(&mut self) -> Result<(), ConnectorError>;
    /// Set the regulatory working area on the device.
    fn set_working_area(&mut self, area: WorkingArea) -> Result<(), ConnectorError>;
    /// Select a tag by EPC for subsequent operations.
    fn select_tag(&mut self, epc: &[u8]) -> Result<(), ConnectorError>;
    /// Clear the current tag selection.
    fn clear_select(&mut self) -> Result<(), ConnectorError>;
    /// Read data from a selected tag's memory bank.
    ///
    /// `access_password` is 4 bytes (use `00000000` unless a non-default
    /// access password is set). `mem_bank` is the Gen2 memory bank:
    /// 0=Reserved, 1=EPC, 2=TID, 3=User. `start_addr` is the starting word
    /// address and `length` is the number of words to read.
    ///
    /// Returns only the requested words; the RSSI/PC/EPC framing of the
    /// reader response is stripped.
    fn read_mem(
        &mut self,
        access_password: &[u8],
        mem_bank: u8,
        start_addr: u16,
        length: u16,
    ) -> Result<Vec<u8>, ConnectorError>;
    /// Read the EPC of the selected tag (convenience over [`SyncIO::read_mem`]).
    fn read_epc(&mut self) -> Result<Vec<u8>, ConnectorError> {
        self.read_mem(&[0x00, 0x00, 0x00, 0x00], 0x01, 0x0002, 6)
    }
    /// Write a new EPC to a tag.
    fn write_epc(&mut self, epc: &[u8]) -> Result<(), ConnectorError>;
    /// Write data to a memory bank of the selected tag.
    ///
    /// The tag must be selected first (see [`SyncIO::select_tag`]). `data`
    /// length must be a multiple of 2 (whole words). `mem_bank` is the Gen2
    /// memory bank: 0=Reserved, 1=EPC, 2=TID, 3=User.
    fn write_mem(
        &mut self,
        access_password: &[u8],
        mem_bank: u8,
        start_addr: u16,
        data: &[u8],
    ) -> Result<(), ConnectorError>;
    /// Kill a previously selected tag.
    ///
    /// The tag must be selected first (see [`SyncIO::select_tag`]). The kill
    /// password is 4 bytes. A tag with the default (all-zero) kill password
    /// cannot be killed — write a non-zero kill password to reserved memory
    /// (bank 0, word 0) first, then pass it here.
    fn kill_tag(&mut self, kill_password: &[u8]) -> Result<(), ConnectorError>;
    /// Lock a previously selected tag.
    ///
    /// The tag must be selected first (see [`SyncIO::select_tag`]). The access
    /// password is 4 bytes and the lock operation is 3 bytes (20-bit
    /// mask/action payload, e.g. `02 00 80` to lock the USER memory bank).
    fn lock_tag(&mut self, access_password: &[u8], lock_data: &[u8]) -> Result<(), ConnectorError>;
    /// Write EPC with automatic select + retry.
    fn write_epc_reliable(
        &mut self,
        current_epc: &[u8],
        new_epc: &[u8],
        max_retries: usize,
    ) -> Result<(), ConnectorError>;
}

impl<S> SyncIO for Connector<S>
where
    S: Read + Write,
{
    type Socket = S;

    /// Setup the reader with default settings (inspired by e710_uhf)
    fn setup_reader(&mut self) -> Result<(), ConnectorError> {
        self.stop_multiple_polling_instructions().ok();
        Ok(())
    }

    fn get_module_info(&mut self) -> Result<String, ConnectorError> {
        self.send_packet(Command::HardwareVersion)?;
        let hardware = self.single_read_from_serial();
        self.send_packet(Command::SoftwareVersion)?;
        let software = self.single_read_from_serial();
        self.send_packet(Command::Manufacturer)?;
        let manufacture = self.single_read_from_serial();

        let out = format!(
            "Hardware: {} - Software: {} - Manufacturer: {}",
            clear_non_ascii(hardware?.unwrap().to_string().as_str()),
            clear_non_ascii(software?.unwrap().to_string().as_str()),
            clear_non_ascii(manufacture?.unwrap().to_string().as_str())
        );

        Ok(out)
    }

    /// Builds and sends the command
    fn send_packet(&mut self, command: Command) -> Result<(), ConnectorError> {
        let frame = Frame::new(&command).to_bytes();

        let mut out = String::new();
        for b in &frame {
            out.push_str(format!("{:02X} ", b).as_str());
        }
        debug!("[TX] {out} - [{command}]");

        self.port.write_all(&frame)?;
        self.port.flush()?;
        Ok(())
    }

    fn single_read_from_serial(&mut self) -> Result<Option<Packet>, ConnectorError> {
        let out = self.read_from_serial(Some(1))?;
        Ok(out.unwrap_or(vec![]).pop())
    }

    fn read_from_serial(
        &mut self,
        num_expected_responses: Option<u32>,
    ) -> Result<Option<Vec<Packet>>, ConnectorError> {
        let mut read_buf: [u8; 1024] = [0u8; 1024];
        let mut rolling: Vec<u8> = Vec::with_capacity(4096);

        let mut output: Vec<Packet> = Vec::new();

        loop {
            let raw_data_size = self.port.read(&mut read_buf);
            debug!("raw_data_size: {:?}", raw_data_size);
            debug!("rolling: {:?}", rolling);
            match raw_data_size {
                Ok(n) if n > 0 => {
                    rolling.extend_from_slice(&read_buf[..n]);

                    debug!("rolling: {:?}", rolling);

                    // print raw for debug
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
                Ok(_) => {
                    // n == 0, nothing
                    return Ok(None);
                }
                Err(ref e) if e.kind() == io::ErrorKind::TimedOut => {
                    // timeout: continue and read again
                    if output.is_empty() {
                        return Err(ConnectorError::Timeout);
                    }
                    break;
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                    // A caught signal (e.g. Ctrl+C) interrupted the blocking read;
                    // retry instead of failing.
                    continue;
                }
                Err(ref e) => {
                    error!("Serial read error: {}", e);
                    return Err(ConnectorError::SerialRead(e.to_string()));
                }
            }
        }
        Ok(Some(output))
    }

    /// Get the current regulatory working area configured on the device.
    ///
    /// Returns
    /// - Ok(WorkingArea) with the region inferred from the device response.
    /// - Err(ConnectorError::InvalidWorkingArea) if the response contains an unknown code.
    /// - Err(ConnectorError::NoPacketReceived) if nothing is received.
    /// - Other ConnectorError variants on I/O failure or timeout.
    fn get_working_area(&mut self) -> Result<WorkingArea, ConnectorError> {
        self.send_packet(Command::GetWorkingArea)?;
        let p = self.single_read_from_serial()?;
        if let Some(p) = p {
            return Connector::<S>::parse_to_working_area(p);
        }
        Err(ConnectorError::NoPacketReceived)
    }

    /// Get the current working RF channel as a frequency in MHz.
    ///
    /// The raw channel index returned by the device is converted to MHz based on
    /// the configured WorkingArea. Different regions use different spacing and base frequencies.
    ///
    /// Returns
    /// - Ok(f64) with the center frequency in MHz.
    /// - Err(ConnectorError::NoPacketReceived) if no response is obtained.
    /// - Other ConnectorError variants on I/O failure, timeout, or unknown working area.
    fn get_working_channel(&mut self) -> Result<f64, ConnectorError> {
        self.send_packet(Command::GetWorkingChannel)?;
        let p = self.single_read_from_serial()?;
        if let Some(p) = p {
            return Ok(self.get_working_area()?.packet_to_64(p));
        }
        Err(ConnectorError::NoPacketReceived)
    }

    /// Read the current transmit power reported by the device.
    ///
    /// The device returns two bytes that represent the power value scaled by 100.
    /// This method combines them and returns the value as f64.
    ///
    /// Returns
    /// - Ok(f64) with the transmit power (device-specific units, typically dBm).
    /// - Err(ConnectorError::NoPacketReceived) if no response is obtained.
    /// - Other ConnectorError variants on I/O failure or timeout.
    fn get_transmit_power(&mut self) -> Result<f64, ConnectorError> {
        self.send_packet(Command::AcquireTransmitPower)?;
        let p = self.single_read_from_serial()?;
        if let Some(p) = p {
            return calculate_transmit_power(p);
        }
        Err(ConnectorError::NoPacketReceived)
    }

    /// Set the transmitter output power.
    ///
    /// Parameters
    /// - power: Desired transmit power in device-specific units (typically dBm).
    ///
    /// Returns
    /// - Ok(()) when the device acknowledges the setting.
    /// - Err(ConnectorError::NoPacketReceived) if no response is obtained.
    /// - Other ConnectorError variants on I/O failure or timeout.
    fn set_transmission_power(&mut self, power: f64) -> Result<(), ConnectorError> {
        self.send_packet(Command::SetTransmissionPower(power))?;
        Connector::<S>::_set_transmission_power(self.single_read_from_serial()?, power)
    }

    /// Perform a single inventory (poll) and return the list of detected tags.
    ///
    /// Sends a SinglePollingInstruction to the reader and parses all returned packets
    /// into a collection of Rfid records containing RSSI, PC, EPC (UID) and CRC.
    ///
    /// Returns
    /// - Ok(`Vec<Rfid>`) possibly empty if no tags are present.
    /// - Err(ConnectorError::Timeout or other) on communication errors.
    fn single_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError> {
        self.send_packet(Command::SinglePollingInstruction)?;
        let response = self.read_from_serial(None)?;
        self.parse_rfid_packets(response)
    }

    fn multi_polling_instruction(&mut self) -> Result<Vec<Rfid>, ConnectorError> {
        self.send_packet(Command::MultiplePollingInstruction(100))?;
        let response = self.read_from_serial(Some(100))?;
        self.parse_rfid_packets(response)
    }

    // Start Multi: AA 00 27 00 03 22 FF FF 4A DD
    fn enable_multiple_polling_instructions(
        &mut self,
        pool_times: u16,
    ) -> Result<(), ConnectorError> {
        self.send_packet(Command::MultiplePollingInstruction(pool_times))
    }

    // Stop Multi: AA 00 28 00 00 28 DD
    fn stop_multiple_polling_instructions(&mut self) -> Result<(), ConnectorError> {
        self.send_packet(Command::StopMultiplePollingInstruction)?;
        // In-flight tag notifications may still arrive alongside the 0x28
        // acknowledgement; keep reading until the acknowledgement is seen.
        match self.read_from_serial(None) {
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

    fn set_working_area(&mut self, area: WorkingArea) -> Result<(), ConnectorError> {
        let code: u8 = match area {
            WorkingArea::China900Mhz => 1,
            WorkingArea::US => 2,
            WorkingArea::EU => 3,
            WorkingArea::China800Mhz => 4,
            WorkingArea::Korea => 6,
        };
        self.send_packet(Command::SetWorkingArea(code))?;
        let p = self.single_read_from_serial()?;
        if let Some(p) = p {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    fn select_tag(&mut self, epc: &[u8]) -> Result<(), ConnectorError> {
        let mut params = Vec::new();
        // SelParam 0x01 (target 3'b000, action 3'b000, MemBank 2'b01 = EPC),
        // Ptr 0x00000020 (bit pointer, not word — EPC bank start), MaskLen
        // 0x60 (6 words = 96 bits), Truncate 0x00 (disabled), then the EPC mask.
        params.push(0x01);
        params.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]);
        params.push(0x60);
        params.push(0x00);
        params.extend_from_slice(epc);
        self.send_packet(Command::SetSelect(params))?;
        check_ack(self.single_read_from_serial()?)?;
        // Select mode 0x02 = send the Select command before every tag operation
        // other than polling inventory (read, write, lock, kill). Without it the
        // module stores the mask but never applies it to tag operations.
        self.send_packet(Command::SetSendSelect(0x02))?;
        check_ack(self.single_read_from_serial()?)?;
        Ok(())
    }

    fn clear_select(&mut self) -> Result<(), ConnectorError> {
        let params = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        self.send_packet(Command::SetSelect(params))?;
        check_ack(self.single_read_from_serial()?)?;
        // Select mode 0x01 = do not send the Select command before tag operations.
        self.send_packet(Command::SetSendSelect(0x01))?;
        check_ack(self.single_read_from_serial()?)?;
        Ok(())
    }

    fn read_mem(
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
        self.send_packet(Command::ReadLabel(params))?;
        let p = self.single_read_from_serial()?;
        if let Some(p) = p {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return strip_read_framing(&p.get_data());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    fn write_epc(&mut self, epc: &[u8]) -> Result<(), ConnectorError> {
        self.write_mem(&[0x00, 0x00, 0x00, 0x00], 0x01, 0x0002, epc)
    }

    fn write_mem(
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
        self.send_packet(Command::WriteLabel(params))?;
        let p = self.single_read_from_serial()?;
        if let Some(p) = p {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    fn kill_tag(&mut self, kill_password: &[u8]) -> Result<(), ConnectorError> {
        let params = kill_password.to_vec();
        self.send_packet(Command::KillTag(params))?;
        let p = self.single_read_from_serial()?;
        if let Some(p) = p {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    fn lock_tag(&mut self, access_password: &[u8], lock_data: &[u8]) -> Result<(), ConnectorError> {
        let mut params = Vec::with_capacity(7);
        params.extend_from_slice(access_password);
        params.extend_from_slice(lock_data);
        self.send_packet(Command::LockTag(params))?;
        let p = self.single_read_from_serial()?;
        if let Some(p) = p {
            if p.is_error() {
                let error_code = ErrorCode::from_byte(p.error_code_byte().unwrap_or(0xFF));
                return Err(ConnectorError::CommandError(error_code));
            }
            return Ok(());
        }
        Err(ConnectorError::NoPacketReceived)
    }

    /// Write EPC to a tag with automatic retry.
    ///
    /// Selects the tag by its current EPC, then writes the new EPC.
    /// On WriteFail, polls to rediscover the tag's actual EPC (writes can be
    /// partially applied, changing the tag's EPC mid-attempt) and retries up to
    /// `max_retries` times.
    fn write_epc_reliable(
        &mut self,
        current_epc: &[u8],
        new_epc: &[u8],
        max_retries: usize,
    ) -> Result<(), ConnectorError> {
        let mut last_err = None;
        let mut select_epc = current_epc.to_vec();

        for attempt in 0..=max_retries {
            self.select_tag(&select_epc)?;
            match self.write_epc(new_epc) {
                Ok(()) => return Ok(()),
                Err(ConnectorError::CommandError(ErrorCode::WriteFail)) => {
                    debug!(
                        "[write_epc] attempt {}/{} failed: WriteFail, polling to rediscover tag",
                        attempt + 1,
                        max_retries + 1
                    );
                    last_err = Some(ConnectorError::CommandError(ErrorCode::WriteFail));
                    // Tag may have partially written EPC; poll to discover actual EPC
                    self.clear_select().ok();
                    if let Ok(tags) = self.single_polling_instruction()
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
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    // Helper: build a device->PC frame with given command code and data bytes
    // cmd: command code for the request
    // param: optional parameter byte (e.g. channel code)
    // data: response data
    //
    fn make_frame(cmd: u8, param: Option<Vec<u8>>, data: &[u8]) -> ResponseType {
        let mut v = Vec::new();
        v.push(R200_FRAME_HEADER);
        v.push(0x01); // frame type: from device to PC (arbitrary for tests)
        v.push(cmd);
        let len = data.len() as u16;
        v.push((len >> 8) as u8);
        v.push((len & 0xFF) as u8);
        v.extend_from_slice(data);
        // checksum: sum of bytes starting at index 1 (type) to last data byte, low 8 bits
        let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
        v.push((sum & 0xFF) as u8);
        v.push(R200_FRAME_END);

        ResponseType::Ok(MockChat {
            request: (cmd, param),
            responses: Ok(v),
        })
    }

    fn make_error_frame(i: io::Error) -> ResponseType {
        ResponseType::Error(i)
    }

    // Build an error response packet carrying the given M100 error code.
    fn make_error_code(code: u8) -> ResponseType {
        let mut v = Vec::new();
        v.push(R200_FRAME_HEADER);
        v.push(0x01);
        v.push(0xFF); // error command
        let data = [code];
        let len = data.len() as u16;
        v.push((len >> 8) as u8);
        v.push((len & 0xFF) as u8);
        v.extend_from_slice(&data);
        let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
        v.push((sum & 0xFF) as u8);
        v.push(R200_FRAME_END);
        ResponseType::Raw(v)
    }

    // Build a 17-byte tag response frame (RSSI + PC + EPC + CRC).
    fn tag_frame(cmd: u8, param: Option<Vec<u8>>, epc: &[u8]) -> ResponseType {
        assert_eq!(epc.len(), 12);
        let mut data = vec![55, 0x30, 0x00];
        data.extend_from_slice(epc);
        data.extend_from_slice(&[0xAB, 0xCD]);
        assert_eq!(data.len(), 17);
        make_frame(cmd, param, &data)
    }

    // Params for select_tag(epc) as sent by the implementation.
    fn select_params(epc: &[u8]) -> Vec<u8> {
        let mut params = Vec::new();
        params.push(0x01);
        params.extend_from_slice(&[0x00, 0x00, 0x00, 0x20]);
        params.push(0x60);
        params.push(0x00);
        params.extend_from_slice(epc);
        params
    }

    // Params for write_epc(epc) as sent by the implementation (bank 1, addr 2).
    fn write_epc_params(epc: &[u8]) -> Vec<u8> {
        let mut params = vec![
            0x00,
            0x00,
            0x00,
            0x00, // access password
            0x01, // bank 1 = EPC
            0x00,
            0x02, // word addr 2
            0x00,
            (epc.len() / 2) as u8, // word len
        ];
        params.extend_from_slice(epc);
        params
    }

    enum ResponseType {
        Ok(MockChat),
        Error(io::Error),
        Raw(Vec<u8>),
    }

    #[derive(Default)]
    struct MockState {
        writes: Vec<Vec<u8>>, // captured writes
        // queue of reads to return on successive read() calls
        chats: Vec<ResponseType>,
    }

    struct MockSerialPort {
        state: Arc<Mutex<MockState>>,
    }

    struct MockChat {
        request: (u8, Option<Vec<u8>>),
        responses: io::Result<Vec<u8>>,
    }

    impl MockSerialPort {
        fn new(chats: Vec<ResponseType>) -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState {
                    writes: vec![],
                    chats,
                })),
            }
        }
    }

    impl Read for MockSerialPort {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut st = self.state.lock().unwrap();

            let writes = st.writes.clone();

            if st.chats.is_empty() {
                // simulate timeout when no more data
                return Err(io::Error::new(io::ErrorKind::TimedOut, "timeout"));
            }
            let next = st.chats.remove(0);

            match next {
                ResponseType::Ok(n) => {
                    if let Some(last_write) = writes.last() {
                        let request_command = last_write[2];

                        // Check the parameter.
                        let parameter_is_valid: bool;

                        if let Some(p) = n.request.1 {
                            // Check that the parameter length is set to 1 (position 4) and
                            // that the parameter is set correctly (position 5).
                            let params = &last_write[5..5 + p.len()];
                            parameter_is_valid = last_write[4] == (p.len() as u8) && p == params;
                        } else {
                            parameter_is_valid = true
                        }

                        if n.request.0 == request_command && parameter_is_valid {
                            match n.responses {
                                Ok(bytes) => {
                                    let n = bytes.len().min(buf.len());
                                    buf[..n].copy_from_slice(&bytes[..n]);
                                    Ok(n)
                                }
                                Err(e) => Err(e),
                            }
                        } else {
                            Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "Sequenza di comandi non prevista",
                            ))
                        }
                    } else {
                        // If no write command was received, we are simply
                        // reading a sequence of frames.
                        let bytes = n.responses.unwrap();
                        let n = bytes.len().min(buf.len());
                        buf[..n].copy_from_slice(&bytes[..n]);
                        Ok(n)
                    }
                }
                ResponseType::Error(e) => Err(e),
                ResponseType::Raw(bytes) => {
                    let n = bytes.len().min(buf.len());
                    buf[..n].copy_from_slice(&bytes[..n]);
                    Ok(n)
                }
            }
        }
    }

    impl Write for MockSerialPort {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut st = self.state.lock().unwrap();
            st.writes.push(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    // ----- Tests -----

    #[test]
    fn test_get_module_info() {
        let hw = make_frame(0x03, Some(vec![0x00]), b"HW1.0");
        let sw = make_frame(0x03, Some(vec![0x01]), b"SW2.0");
        let mf = make_frame(0x03, Some(vec![0x02]), b"ACME");
        let mock = MockSerialPort::new(vec![hw, sw, mf]);
        let mut connector = Connector::new(mock);

        let info = connector.get_module_info().unwrap();
        assert!(info.contains("Hardware: HW1.0"));
        assert!(info.contains("Software: SW2.0"));
        assert!(info.contains("Manufacturer: ACME"));
    }

    #[test]
    fn test_get_working_area_mapping() {
        for (code, expected) in [
            (1, WorkingArea::China900Mhz),
            (2, WorkingArea::US),
            (3, WorkingArea::EU),
            (4, WorkingArea::China800Mhz),
            (6, WorkingArea::Korea),
        ] {
            let frame = make_frame(0x08, None, &[code]);
            let mock = MockSerialPort::new(vec![frame]);
            let mut connector = Connector::new(mock);
            let area = connector.get_working_area().unwrap();
            // Compare by variant name via debug
            assert_eq!(format!("{:?}", area), format!("{:?}", expected));
        }
    }

    #[test]
    fn test_get_working_channel_uses_area() {
        // Channel index 4 -> depends on area. We'll test EU mapping: 0.2 MHz step + 865.1
        // First response: channel index, Second: area code 3 (EU)
        let chan = make_frame(0xAA, None, &[4]);
        let area = make_frame(0x08, None, &[3]);
        let mock = MockSerialPort::new(vec![chan, area]);
        let mut connector = Connector::new(mock);
        let freq = connector.get_working_channel().unwrap();
        assert!((freq - (4.0 * 0.2 + 865.1)).abs() < 1e-6);
    }

    #[test]
    fn test_get_transmit_power() {
        // 27.50 -> 2750 -> 0x0A BE (for example 0x0A, 0xBE => 2750)
        let frame = make_frame(0xB7, None, &[0x0A, 0xBE]);
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        let p = connector.get_transmit_power().unwrap();
        assert!((p - 27.50).abs() < 1e-6);
    }

    #[test]
    fn test_set_transmission_power_ack() {
        // ACK byte 0x00
        let frame = make_frame(0xB6, Some(vec![0x07, 0xD0]), &[0x00]);
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        connector.set_transmission_power(20.0).unwrap();
    }

    #[test]
    fn test_kill_tag_success() {
        // Response frame for kill (0x65) with no data = success
        let frame = make_frame(0x65, Some(vec![0x00, 0x00, 0xFF, 0xFF]), &[]);
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        connector.kill_tag(&[0x00, 0x00, 0xFF, 0xFF]).unwrap();
    }

    #[test]
    fn test_kill_tag_kill_fail_error() {
        // Error packet with code 0x12 = KillFail (command 0xFF)
        let err_packet = {
            let mut v = Vec::new();
            v.push(R200_FRAME_HEADER);
            v.push(0x01);
            v.push(0xFF); // error command
            let data = [0x12u8]; // KillFail
            let len = data.len() as u16;
            v.push((len >> 8) as u8);
            v.push((len & 0xFF) as u8);
            v.extend_from_slice(&data);
            let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
            v.push((sum & 0xFF) as u8);
            v.push(R200_FRAME_END);
            ResponseType::Raw(v)
        };
        let mock = MockSerialPort::new(vec![err_packet]);
        let mut connector = Connector::new(mock);
        let err = connector.kill_tag(&[0x00, 0x00, 0xFF, 0xFF]).unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::KillFail)
        ));
    }

    #[test]
    fn test_lock_tag_success() {
        // Lock (0x82) with access password + lock data params, empty response = success
        let frame = make_frame(
            0x82,
            Some(vec![0x00, 0x00, 0xFF, 0xFF, 0x02, 0x00, 0x80]),
            &[],
        );
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        connector
            .lock_tag(&[0x00, 0x00, 0xFF, 0xFF], &[0x02, 0x00, 0x80])
            .unwrap();
    }

    #[test]
    fn test_lock_tag_lock_fail_error() {
        // Error packet with code 0x13 = LockFail
        let err_packet = {
            let mut v = Vec::new();
            v.push(R200_FRAME_HEADER);
            v.push(0x01);
            v.push(0xFF);
            let data = [0x13u8]; // LockFail
            let len = data.len() as u16;
            v.push((len >> 8) as u8);
            v.push((len & 0xFF) as u8);
            v.extend_from_slice(&data);
            let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
            v.push((sum & 0xFF) as u8);
            v.push(R200_FRAME_END);
            ResponseType::Raw(v)
        };
        let mock = MockSerialPort::new(vec![err_packet]);
        let mut connector = Connector::new(mock);
        let err = connector
            .lock_tag(&[0x00, 0x00, 0xFF, 0xFF], &[0x02, 0x00, 0x80])
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::LockFail)
        ));
    }

    #[test]
    fn test_read_mem_success() {
        // Read 2 words from reserved bank (0), addr 0, with access password.
        // Response payload: ul=14 (PC 2 + EPC 12), PC 0x3000, 12-byte EPC,
        // then the 4 requested data bytes.
        let mut resp = vec![0x0E];
        resp.extend_from_slice(&[0x30, 0x00]);
        resp.extend_from_slice(&[0u8; 12]);
        resp.extend_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        let frame = make_frame(
            0x39,
            Some(vec![0x12, 0x34, 0x56, 0x78, 0x00, 0x00, 0x00, 0x00, 0x02]),
            &resp,
        );
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        let data = connector
            .read_mem(&[0x12, 0x34, 0x56, 0x78], 0, 0, 2)
            .unwrap();
        assert_eq!(data, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_read_mem_error() {
        // Error packet with code 0x10 (WriteFail), reused as generic command error
        let err_packet = {
            let mut v = Vec::new();
            v.push(R200_FRAME_HEADER);
            v.push(0x01);
            v.push(0xFF);
            let data = [0x10u8];
            let len = data.len() as u16;
            v.push((len >> 8) as u8);
            v.push((len & 0xFF) as u8);
            v.extend_from_slice(&data);
            let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
            v.push((sum & 0xFF) as u8);
            v.push(R200_FRAME_END);
            ResponseType::Raw(v)
        };
        let mock = MockSerialPort::new(vec![err_packet]);
        let mut connector = Connector::new(mock);
        let err = connector
            .read_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, 2)
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::WriteFail)
        ));
    }

    #[test]
    fn test_read_mem_truncated_response() {
        // Response shorter than ul header promises: ul=20 but only 19 bytes total
        let mut resp = vec![20u8];
        resp.extend_from_slice(&[0u8; 18]);
        let frame = make_frame(
            0x39,
            Some(vec![0u8; 7].into_iter().chain([0x00, 0x02]).collect()),
            &resp,
        );
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        let err = connector
            .read_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, 2)
            .unwrap_err();
        assert!(
            matches!(err, ConnectorError::InvalidResponse(_)),
            "unexpected error: {:?}",
            err
        );
    }

    #[test]
    fn test_write_mem_success() {
        // Write to reserved bank (0), addr 0, 2 words (4 bytes): 00 00 00 00
        let frame = make_frame(
            0x49,
            Some(vec![
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00,
            ]),
            &[],
        );
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        connector
            .write_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, &[0x00, 0x00, 0x00, 0x00])
            .unwrap();
    }

    #[test]
    fn test_write_mem_odd_length_rejected() {
        let mock = MockSerialPort::new(vec![]);
        let mut connector = Connector::new(mock);
        let err = connector
            .write_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, &[0x00, 0x01, 0x02])
            .unwrap_err();
        assert!(matches!(err, ConnectorError::FailedSetting(_)));
    }

    #[test]
    fn test_write_mem_write_fail_error() {
        // Error packet with code 0x10 = WriteFail
        let err_packet = {
            let mut v = Vec::new();
            v.push(R200_FRAME_HEADER);
            v.push(0x01);
            v.push(0xFF);
            let data = [0x10u8]; // WriteFail
            let len = data.len() as u16;
            v.push((len >> 8) as u8);
            v.push((len & 0xFF) as u8);
            v.extend_from_slice(&data);
            let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
            v.push((sum & 0xFF) as u8);
            v.push(R200_FRAME_END);
            ResponseType::Raw(v)
        };
        let mock = MockSerialPort::new(vec![err_packet]);
        let mut connector = Connector::new(mock);
        let err = connector
            .write_mem(&[0x00, 0x00, 0x00, 0x00], 0, 0, &[0x00, 0x00, 0x00, 0x00])
            .unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::WriteFail)
        ));
    }

    #[test]
    fn test_single_polling_instruction_parses_tags() {
        // Build two tag frames then a timeout to end collection
        let tag1 = {
            let data = vec![
                55, // RSSI
                0x30, 0x12, // PC = 0x3012
                0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
                0x08, // padding to reach index 15
                0xAB, 0xCD, // CRC bytes at 15,16
            ];
            make_frame(0x22, None, &data)
        };
        let tag2 = {
            let data = vec![
                60, 0x20, 0x34, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB,
                0xCC, 0x12, 0x34,
            ];
            make_frame(0x22, None, &data)
        };
        let timeout = make_error_frame(io::Error::new(io::ErrorKind::TimedOut, "done"));
        let mock = MockSerialPort::new(vec![tag1, tag2, timeout]);
        let mut connector = Connector::new(mock);
        let tags = connector.single_polling_instruction().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].uid(), "DEADBEEF0102030405060708");
    }

    #[test]
    fn test_read_from_serial_noise_and_multiple_frames() {
        // Noise bytes, then two frames in one read, then timeout to finish
        let noise = vec![0x00, 0xFF, 0x13, 0x37];
        let f1 = make_frame(0x08, None, &[2]);
        let f2 = make_frame(0xAA, None, &[7]);
        let mock = MockSerialPort::new(vec![
            ResponseType::Raw(noise),
            f1,
            f2,
            make_error_frame(io::Error::new(io::ErrorKind::TimedOut, "t")),
        ]);
        let mut connector = Connector::new(mock);
        let out = connector.read_from_serial(None).unwrap().unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].get_data(), vec![2]);
        assert_eq!(out[1].get_data(), vec![7]);
    }

    // ---- clear_non_ascii tests ----

    #[test]
    fn test_clear_non_ascii_ascii_only() {
        let s = "Hello, World! 123";
        let out = clear_non_ascii(s);
        assert_eq!(out, s);
    }

    #[test]
    fn test_clear_non_ascii_removes_non_ascii() {
        // Mixed ASCII + non-ASCII (Euro sign, CJK, and 'ç')
        let s = "a€b測cçd";
        let out = clear_non_ascii(s);
        assert_eq!(out, "abcd");
    }

    #[test]
    fn test_clear_non_ascii_keeps_ascii_control_chars() {
        let s = "A\nB\tC\r\x07"; // includes newline, tab, carriage return, BEL
        let out = clear_non_ascii(s);
        assert_eq!(out, s);
    }

    #[test]
    fn test_clear_non_ascii_empty_input() {
        let s = "";
        let out = clear_non_ascii(s);
        assert_eq!(out, "");
    }

    // ----- setup / select / working area / polling / reliable write tests -----

    #[test]
    fn test_setup_reader_stops_polling() {
        let stop = make_frame(0x28, None, &[]);
        let mock = MockSerialPort::new(vec![stop]);
        let mut connector = Connector::new(mock);
        connector.setup_reader().unwrap();
    }

    #[test]
    fn test_setup_reader_ignores_failure() {
        // No chats: the stop-polling read times out but setup_reader ignores it.
        let mock = MockSerialPort::new(vec![]);
        let mut connector = Connector::new(mock);
        connector.setup_reader().unwrap();
    }

    #[test]
    fn test_select_tag_success() {
        let epc = [
            0xE0, 0x28, 0x06, 0x91, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mask = make_frame(0x0C, Some(select_params(&epc)), &[]);
        let mode = make_frame(0x12, Some(vec![0x02]), &[]);
        let mock = MockSerialPort::new(vec![mask, mode]);
        let mut connector = Connector::new(mock);
        connector.select_tag(&epc).unwrap();
    }

    #[test]
    fn test_select_tag_errors_on_send_select() {
        let epc = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mask = make_frame(0x0C, Some(select_params(&epc)), &[]);
        let mode_err = make_error_code(0x16); // AccessFail
        let mock = MockSerialPort::new(vec![mask, mode_err]);
        let mut connector = Connector::new(mock);
        let err = connector.select_tag(&epc).unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::AccessFail)
        ));
    }

    #[test]
    fn test_clear_select_success() {
        let mask = make_frame(0x0C, Some(vec![0u8; 8]), &[]);
        let mode = make_frame(0x12, Some(vec![0x01]), &[]);
        let mock = MockSerialPort::new(vec![mask, mode]);
        let mut connector = Connector::new(mock);
        connector.clear_select().unwrap();
    }

    // Build a raw tag frame (RSSI + PC + EPC + CRC) as delivered by a poll.
    fn raw_tag_frame(epc: &[u8], crc: [u8; 2]) -> Vec<u8> {
        assert_eq!(epc.len(), 12);
        let mut v = Vec::new();
        v.push(R200_FRAME_HEADER);
        v.push(0x01);
        v.push(0x22); // Single Polling response
        let mut data = vec![55, 0x30, 0x00];
        data.extend_from_slice(epc);
        data.extend_from_slice(&crc);
        let len = data.len() as u16;
        v.push((len >> 8) as u8);
        v.push((len & 0xFF) as u8);
        v.extend_from_slice(&data);
        let sum: u16 = v[1..].iter().map(|&b| b as u16).sum();
        v.push((sum & 0xFF) as u8);
        v.push(R200_FRAME_END);
        v
    }

    #[test]
    fn test_parser_handles_0xdd_inside_crc() {
        // Tag E28068910000000000000002 has CRC BD DD; the 0xDD in the CRC is not a
        // frame terminator. The parser must use the PL length field to find the end.
        let epc = [0xE2, 0x80, 0x68, 0x91, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02];
        let frame = raw_tag_frame(&epc, [0xBD, 0xDD]);
        let mock = MockSerialPort::new(vec![ResponseType::Raw(frame)]);
        let mut connector = Connector::new(mock);
        let packets = connector.read_from_serial(None).unwrap().unwrap();
        assert_eq!(packets.len(), 1);
        assert!(packets[0].is_valid());
        assert_eq!(packets[0].get_data(), vec![55, 0x30, 0x00, 0xE2, 0x80, 0x68, 0x91, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xBD, 0xDD]);
    }

    #[test]
    fn test_parser_handles_multiple_frames_in_one_read() {
        // Two tag frames (one with a 0xDD-carrying CRC) concatenated in a single read.
        let f1 = raw_tag_frame(
            &[0xE2, 0x80, 0x68, 0x91, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01],
            [0x8D, 0xBE],
        );
        let f2 = raw_tag_frame(
            &[0xE2, 0x80, 0x68, 0x91, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02],
            [0xBD, 0xDD],
        );
        let mut both = f1;
        both.extend_from_slice(&f2);
        let mock = MockSerialPort::new(vec![ResponseType::Raw(both)]);
        let mut connector = Connector::new(mock);
        let packets = connector.read_from_serial(Some(2)).unwrap().unwrap();
        assert_eq!(packets.len(), 2);
        assert!(packets.iter().all(|p| p.is_valid()));
    }

    #[test]
    fn test_set_working_area_success() {
        let frame = make_frame(0x07, Some(vec![3]), &[]);
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        connector.set_working_area(WorkingArea::EU).unwrap();
    }

    #[test]
    fn test_set_working_area_error() {
        let err = make_error_code(0x16); // AccessFail
        let mock = MockSerialPort::new(vec![err]);
        let mut connector = Connector::new(mock);
        let err = connector.set_working_area(WorkingArea::EU).unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::AccessFail)
        ));
    }

    #[test]
    fn test_multi_polling_instruction_parses_tags() {
        let epc1 = [
            0xE0, 0x28, 0x06, 0x91, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        ];
        let epc2 = [
            0xE0, 0x28, 0x06, 0x91, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
        ];
        let tag1 = tag_frame(0x27, Some(vec![0x22, 0x00, 0x64]), &epc1);
        let tag2 = tag_frame(0x27, Some(vec![0x22, 0x00, 0x64]), &epc2);
        let timeout = make_error_frame(io::Error::new(io::ErrorKind::TimedOut, "done"));
        let mock = MockSerialPort::new(vec![tag1, tag2, timeout]);
        let mut connector = Connector::new(mock);
        let tags = connector.multi_polling_instruction().unwrap();
        assert_eq!(tags.len(), 2);
        let expected: String = epc1.iter().map(|b| format!("{:02X}", b)).collect();
        assert_eq!(tags[0].uid(), expected);
    }

    #[test]
    fn test_stop_multiple_polling_instructions_success() {
        let frame = make_frame(0x28, None, &[]);
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        connector.stop_multiple_polling_instructions().unwrap();
    }

    #[test]
    fn test_stop_multiple_polling_wrong_response() {
        // Error packet (cmd 0xFF) doesn't map to StopMultiplePollingInstruction.
        let err = make_error_code(0x12);
        let mock = MockSerialPort::new(vec![err]);
        let mut connector = Connector::new(mock);
        let err = connector.stop_multiple_polling_instructions().unwrap_err();
        assert!(matches!(err, ConnectorError::ErrorStopMultiPolling(_)));
    }

    #[test]
    fn test_write_epc_success() {
        let epc = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x00];
        let frame = make_frame(0x49, Some(write_epc_params(&epc)), &[]);
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        connector.write_epc(&epc).unwrap();
    }

    #[test]
    fn test_read_epc_convenience() {
        // read_epc() = read_mem(0000, bank 1, addr 2, 6 words).
        // Response: ul=14, PC(2), EPC(12), then 12 requested bytes.
        let mut resp = vec![0x0E];
        resp.extend_from_slice(&[0x30, 0x00]);
        resp.extend_from_slice(&[0u8; 12]);
        resp.extend_from_slice(&[
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x01, 0x02, 0x03,
        ]);
        let params = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00, 0x06];
        let frame = make_frame(0x39, Some(params), &resp);
        let mock = MockSerialPort::new(vec![frame]);
        let mut connector = Connector::new(mock);
        let data = connector.read_epc().unwrap();
        assert_eq!(
            data,
            vec![
                0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x01, 0x02, 0x03
            ]
        );
    }

    #[test]
    fn test_write_epc_reliable_success_first_try() {
        let current = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x00];
        let new = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x01];
        let select = make_frame(0x0C, Some(select_params(&current)), &[]);
        let mode = make_frame(0x12, Some(vec![0x02]), &[]);
        let write = make_frame(0x49, Some(write_epc_params(&new)), &[]);
        let mock = MockSerialPort::new(vec![select, mode, write]);
        let mut connector = Connector::new(mock);
        connector.write_epc_reliable(&current, &new, 1).unwrap();
    }

    #[test]
    fn test_write_epc_reliable_rediscover_and_retry() {
        let current = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x00];
        // 12-byte EPC the tag actually ends up with after the partial write.
        let rediscovered = [
            0xE0, 0x28, 0x06, 0x91, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let new = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x01];

        // attempt 0: select ok, then write -> WriteFail (0x10)
        let select0 = make_frame(0x0C, Some(select_params(&current)), &[]);
        let mode0 = make_frame(0x12, Some(vec![0x02]), &[]);
        let write_fail = make_error_code(0x10);

        // rediscovery: clear select ok, poll returns the new EPC, then timeout
        let clear_mask = make_frame(0x0C, Some(vec![0u8; 8]), &[]);
        let clear_mode = make_frame(0x12, Some(vec![0x01]), &[]);
        let poll = tag_frame(0x22, None, &rediscovered);
        let timeout = make_error_frame(io::Error::new(io::ErrorKind::TimedOut, "done"));

        // attempt 1: select with rediscovered EPC, write ok
        let select1 = make_frame(0x0C, Some(select_params(&rediscovered)), &[]);
        let mode1 = make_frame(0x12, Some(vec![0x02]), &[]);
        let write_ok = make_frame(0x49, Some(write_epc_params(&new)), &[]);

        let mock = MockSerialPort::new(vec![
            select0,
            mode0,
            write_fail,
            clear_mask,
            clear_mode,
            poll,
            timeout,
            select1,
            mode1,
            write_ok,
        ]);
        let mut connector = Connector::new(mock);
        connector.write_epc_reliable(&current, &new, 1).unwrap();
    }

    #[test]
    fn test_write_epc_reliable_exhausts_retries() {
        let current = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x00];
        let new = [0xE0, 0x28, 0x06, 0x91, 0x05, 0x01];
        let mock = MockSerialPort::new(vec![
            make_frame(0x0C, Some(select_params(&current)), &[]),
            make_frame(0x12, Some(vec![0x02]), &[]),
            make_error_code(0x10),
        ]);
        let mut connector = Connector::new(mock);
        // max_retries=0 -> a single WriteFail attempt, no retry
        let err = connector.write_epc_reliable(&current, &new, 0).unwrap_err();
        assert!(matches!(
            err,
            ConnectorError::CommandError(ErrorCode::WriteFail)
        ));
    }

    #[test]
    fn test_no_packet_received() {
        // Read returns Ok(0) -> read_from_serial yields Ok(None) -> NoPacketReceived.
        let mock = MockSerialPort::new(vec![ResponseType::Raw(vec![])]);
        let mut connector = Connector::new(mock);
        let err = connector.get_working_area().unwrap_err();
        assert!(matches!(err, ConnectorError::NoPacketReceived));
    }
}

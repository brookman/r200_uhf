pub mod sync;

#[cfg(feature = "async")]
mod async_impl;

#[cfg(feature = "async")]
pub use async_impl::*;

use crate::Rfid;
use crate::frame::ErrorCode;
use crate::packet::Packet;
use log::{debug, error, info};
use std::fmt;
use std::io;

pub struct Connector<P> {
    port: P,
}

impl<P> Connector<P> {
    /// Create a new Connector wrapping an open serial port.
    pub fn new(port: P) -> Self {
        Connector { port }
    }

    fn parse_to_working_area(p: Packet) -> Result<WorkingArea, ConnectorError> {
        let data = p.get_data();
        if data.is_empty() {
            return Err(ConnectorError::InvalidResponse(
                "Empty working area response".into(),
            ));
        }
        match data[0] {
            1 => Ok(WorkingArea::China900Mhz),
            2 => Ok(WorkingArea::US),
            3 => Ok(WorkingArea::EU),
            4 => Ok(WorkingArea::China800Mhz),
            6 => Ok(WorkingArea::Korea),
            _ => Err(ConnectorError::InvalidWorkingArea),
        }
    }

    fn _set_transmission_power(p: Option<Packet>, power: f64) -> Result<(), ConnectorError> {
        if let Some(p) = p {
            let data = p.get_data();
            if data.is_empty() {
                return Err(ConnectorError::InvalidResponse(
                    "Empty set-power ACK".into(),
                ));
            }
            if data[0] == 0x00 {
                info!("Power correct set to {}", power);
                return Ok(());
            } else {
                error!("Power not set to {}", power);
                return Err(ConnectorError::FailedSetting(format!(
                    "Transmission power not set to {}",
                    power
                )));
            }
        }
        Err(ConnectorError::NoPacketReceived)
    }

    fn parse_rfid_packets(
        &self,
        response: Option<Vec<Packet>>,
    ) -> Result<Vec<Rfid>, ConnectorError> {
        let mut rfids = Vec::new();
        if let Some(ps) = response {
            if ps.len() == 1 && ps[0].get_data().first() == Some(&0x15) {
                debug!("No tags present");
            } else {
                for p in ps {
                    let data = p.get_data();
                    if data.len() == 17 {
                        rfids.push(Rfid::from_raw(data));
                    }
                }
            }
        }
        Ok(rfids)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Regulatory working area (RF band) configured on the reader.
pub enum WorkingArea {
    China900Mhz,
    China800Mhz,
    US,
    EU,
    Korea,
}

impl WorkingArea {
    /// Convert a channel-index response packet into a frequency in MHz for this area.
    pub fn packet_to_64(&self, p: Packet) -> f64 {
        let data = p.get_data();
        if data.is_empty() {
            return 0.0;
        }
        match self {
            WorkingArea::China900Mhz => (data[0] as f64) * 0.25 + 920.125,
            WorkingArea::China800Mhz => (data[0] as f64) * 0.25 + 840.125,
            WorkingArea::US => (data[0] as f64) * 0.50 + 902.25,
            WorkingArea::EU => (data[0] as f64) * 0.2 + 865.1,
            WorkingArea::Korea => (data[0] as f64) * 0.2 + 917.1,
        }
    }
}

/// Errors produced by the R200 protocol layer.
#[derive(Debug)]
pub enum ConnectorError {
    /// Underlying serial I/O failure.
    Io(io::Error),
    /// Serial timeout while waiting for a response.
    Timeout,
    /// Reader reported an unknown working area code.
    InvalidWorkingArea,
    /// No response packet was received.
    NoPacketReceived,
    /// A setting could not be applied.
    FailedSetting(String),
    /// A response had an unexpected shape or contents.
    InvalidResponse(String),
    /// A serial read failed unexpectedly.
    SerialRead(String),
    /// Stopping multiple polling failed.
    ErrorStopMultiPolling(String),
    /// The reader returned a protocol error code.
    CommandError(ErrorCode),
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectorError::Io(e) => write!(f, "IO error: {}", e),
            ConnectorError::Timeout => write!(f, "Timeout"),
            ConnectorError::InvalidWorkingArea => write!(f, "Invalid working area"),
            ConnectorError::NoPacketReceived => write!(f, "No packet received"),
            ConnectorError::SerialRead(msg) => write!(f, "Serial read error: {}", msg),
            ConnectorError::FailedSetting(msg) => write!(f, "Failed Setting: {}", msg),
            ConnectorError::InvalidResponse(msg) => write!(f, "Invalid response: {}", msg),
            ConnectorError::ErrorStopMultiPolling(msg) => {
                write!(f, "Impossible to stop multiple polling [{msg}]")
            }
            ConnectorError::CommandError(code) => write!(f, "Command error: {}", code),
        }
    }
}

impl std::error::Error for ConnectorError {}

impl From<io::Error> for ConnectorError {
    fn from(err: io::Error) -> Self {
        ConnectorError::Io(err)
    }
}

/// Strip the RSSI/PC/EPC framing from a ReadData response.
///
/// The response payload is laid out as:
/// - `[0]` = `ul`, the byte length of the PC + EPC section
/// - `[1..1+ul]` = PC (2 bytes) + EPC (`ul - 2` bytes)
/// - `[1+ul..]` = the requested memory words
pub(crate) fn strip_read_framing(data: &[u8]) -> Result<Vec<u8>, ConnectorError> {
    let Some(&ul) = data.first() else {
        return Err(ConnectorError::InvalidResponse(
            "Empty read data response".into(),
        ));
    };
    let ul = ul as usize;
    if data.len() < 1 + ul {
        return Err(ConnectorError::InvalidResponse(
            "Truncated read data response".into(),
        ));
    }
    Ok(data[1 + ul..].to_vec())
}

pub(crate) fn clear_non_ascii(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii()).collect()
}

pub(crate) fn hexdump_line(prefix: &str, data: &[u8]) {
    let mut out = String::new();
    for b in data {
        out.push_str(format!("{:02X} ", b).as_str());
    }
    log::debug!("{} {}", prefix, out);
}

pub(crate) fn parse_hex_str(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0))
        .collect()
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub(crate) fn calculate_transmit_power(p: Packet) -> Result<f64, ConnectorError> {
    let data = p.get_data();
    if data.len() >= 2 {
        Ok(((data[0] as u16) * 256 + (data[1] as u16)) as f64 / 100.0)
    } else if data.len() == 1 {
        Ok(data[0] as f64)
    } else {
        Err(ConnectorError::InvalidResponse(
            "Empty power response".into(),
        ))
    }
}

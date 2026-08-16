//! Serial protocol library for R200 UHF RFID reader modules (e.g. M100).
//!
//! Provides a [`Connector`](connector::Connector) API over a serial port for
//! device info, RF settings, tag inventory, and tag memory access (read/write,
//! select, lock, kill).
//!
//! Two trait families are available:
//! - [`sync::SyncIO`](connector::sync::SyncIO) for blocking usage.
//! - [`async::AsyncIO`](connector::AsyncIO) (feature `async`) for `tokio`.
//!
//! ```no_run
//! use r200_uhf::connector::Connector;
//! use r200_uhf::connector::sync::SyncIO;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let port = serialport::new("/dev/ttyUSB0", 115200).open()?;
//! let mut conn = Connector::new(port);
//! let tags = conn.single_polling_instruction()?;
//! for tag in tags {
//!     println!("{}", tag.uid());
//! }
//! # Ok(())
//! # }
//! ```

pub mod connector;

#[cfg(feature = "cli")]
pub mod cli;

mod frame;
mod packet;
mod rfid;

pub use rfid::Rfid;

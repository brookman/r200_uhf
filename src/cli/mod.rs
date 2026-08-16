//! R200 UHF RFID reader command-line interface.
//!
//! Enabled by the `cli` cargo feature, which also builds the `r200` binary in
//! `src/main.rs`. This module is a thin wrapper around the blocking
//! [`SyncIO`](crate::connector::sync::SyncIO) API.

mod display;
mod port;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::connector::sync::SyncIO;
use crate::connector::WorkingArea;
use display::{display_tag, hex};
use port::{ReaderGuard, SharedPort};

/// Parse a hex string (with optional `0x` prefix) into bytes.
pub fn parse_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if !s.len().is_multiple_of(2) {
        anyhow::bail!("Hex string must have even length");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Invalid hex string: {}", e))
}

/// Open the serial port and return a [`Connector`](crate::connector::Connector)
/// plus a reader guard that stops multi-polling on drop.
pub fn connect(
    port_name: &str,
    baud_rate: u32,
) -> Result<(crate::connector::Connector<SharedPort>, ReaderGuard)> {
    let port = serialport::new(port_name, baud_rate)
        .timeout(Duration::from_millis(500))
        .open()?;
    let shared = SharedPort::new(port);
    shared.clear_input()?;
    let guard = ReaderGuard::new(shared.clone());
    let connector = crate::connector::Connector::new(shared);
    Ok((connector, guard))
}

#[derive(Parser)]
#[command(name = "r200", about = "R200 UHF RFID reader")]
struct Cli {
    /// Serial port device path (required)
    #[arg(long)]
    port: String,

    /// Serial port baud rate
    #[arg(long, default_value = "115200")]
    baud: u32,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show module info and current region
    Info,
    /// Poll for a tag (waits until found)
    Poll,
    /// Continuous tag scanning (no duplicates)
    Scan,
    /// Write EPC to a tag
    Write {
        /// 12-byte EPC in hex (24 chars)
        epc: String,
        /// Only write to tag with this EPC
        #[arg(long)]
        select: Option<String>,
    },
    /// Write data to any memory bank of a tag
    WriteMem {
        /// Data bytes in hex (even number of chars)
        data: String,
        /// Memory bank: 0=Reserved, 1=EPC, 2=TID, 3=User
        #[arg(long, default_value = "0")]
        bank: u8,
        /// Word address to start writing at
        #[arg(long, default_value = "0")]
        addr: u16,
        /// 4-byte access password in hex (8 chars, default 00000000)
        #[arg(long, default_value = "00000000")]
        access: String,
        /// Only write to tag with this EPC
        #[arg(long)]
        select: Option<String>,
    },
    /// Kill a tag (destroys it permanently)
    Kill {
        /// 4-byte kill password in hex (8 chars, default 00000000)
        #[arg(long, default_value = "00000000")]
        password: String,
        /// Only kill tag with this EPC
        #[arg(long)]
        select: Option<String>,
    },
    /// Lock a tag's memory banks
    Lock {
        /// 4-byte access password in hex (8 chars, default 00000000)
        #[arg(long, default_value = "00000000")]
        password: String,
        /// 3-byte lock operation in hex (6 chars), e.g. 020080 to lock USER memory
        #[arg(long, default_value = "020080")]
        lock_data: String,
        /// Only lock tag with this EPC
        #[arg(long)]
        select: Option<String>,
    },
    /// Read memory from a tag
    Read {
        /// Memory bank: 0=Reserved, 1=EPC, 2=TID, 3=User
        #[arg(long, default_value = "1")]
        bank: u8,
        /// Word address to start reading from
        #[arg(long, default_value = "0")]
        addr: u16,
        /// Number of words to read
        #[arg(long, default_value = "6")]
        length: u16,
        /// 4-byte access password in hex (8 chars, default 00000000)
        #[arg(long, default_value = "00000000")]
        access: String,
        /// Only read tag with this EPC
        #[arg(long)]
        select: Option<String>,
    },
    /// Set the RF region
    Region {
        #[arg(value_parser = ["china900", "china800", "eu", "us", "korea"])]
        area: String,
    },
    /// Get or set transmit power (dBm)
    Power {
        /// Power level in dBm (omit to just display current)
        level: Option<f64>,
    },
}

/// Run the CLI. Entry point of the `r200` binary.
pub fn run() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    let (mut connector, _guard) =
        connect(&cli.port, cli.baud).context("Failed to open serial port")?;

    match cli.command {
        Commands::Info => {
            let info = connector.get_module_info()?;
            println!("Module: {}", info);
            let area = connector.get_working_area()?;
            println!("Region: {:?}", area);
            if let Ok(ch) = connector.get_working_channel() {
                println!("Channel: {} MHz", ch);
            }
            if let Ok(pow) = connector.get_transmit_power() {
                println!("Power:   {} dBm", pow);
            }
        }

        Commands::Poll => {
            println!("Polling for tags... (place a tag near the antenna)");
            loop {
                let tags = connector.single_polling_instruction()?;
                if let Some(tag) = tags.first() {
                    display_tag(tag);
                    break;
                }
            }
        }

        Commands::Scan => {
            println!("Scanning for tags... (Ctrl+C to stop)");
            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();
            ctrlc::set_handler(move || {
                r.store(false, Ordering::SeqCst);
            })
            .context("Ctrl+C handler")?;
            let mut seen = HashSet::new();
            while running.load(Ordering::SeqCst) {
                if let Ok(tags) = connector.multi_polling_instruction() {
                    for tag in &tags {
                        if seen.insert(tag.epc.clone()) {
                            if seen.len() > 1 {
                                println!("  ---");
                            }
                            display_tag(tag);
                        }
                    }
                }
            }
        }

        Commands::Write { epc, select } => {
            let epc_bytes = parse_hex(&epc)?;
            if epc_bytes.len() != 12 {
                anyhow::bail!("EPC must be exactly 24 hex characters (12 bytes)");
            }
            let select_bytes = match &select {
                Some(s) => {
                    let b = parse_hex(s)?;
                    if b.len() != 12 {
                        anyhow::bail!("--select EPC must be exactly 24 hex characters");
                    }
                    Some(b)
                }
                None => None,
            };

            println!("Writing EPC: {}", epc);

            println!("Place a tag on the antenna...");
            let current_epc = loop {
                let tags = connector.single_polling_instruction()?;
                if let Some(tag) = tags.first() {
                    break parse_hex(&tag.epc)?;
                }
                std::thread::sleep(Duration::from_millis(100));
            };

            let select_epc = select_bytes.as_ref().unwrap_or(&current_epc);
            println!("Selecting tag: {}", hex(select_epc));
            println!("Tag detected, writing...");
            connector.write_epc_reliable(select_epc, &epc_bytes, 3)?;
            connector.clear_select()?;
            let tags = connector.single_polling_instruction()?;
            if let Some(tag) = tags.first() {
                if tag.epc.eq_ignore_ascii_case(&hex(&epc_bytes)) {
                    println!("  Verified: {}", tag.epc);
                } else {
                    println!(
                        "Warning: tag reads as {}, expected {}",
                        tag.epc,
                        hex(&epc_bytes)
                    );
                }
            } else {
                println!("Warning: could not read back tag for verification.");
            }
        }

        Commands::WriteMem {
            data,
            bank,
            addr,
            access,
            select,
        } => {
            let data_bytes = parse_hex(&data)?;
            if data_bytes.is_empty() || data_bytes.len() % 2 != 0 {
                anyhow::bail!("Data must be a non-empty even number of hex characters");
            }
            let access_bytes = parse_hex(&access)?;
            if access_bytes.len() != 4 {
                anyhow::bail!("--access must be exactly 8 hex characters (4 bytes)");
            }
            let select_bytes = match &select {
                Some(s) => {
                    let b = parse_hex(s)?;
                    if b.len() != 12 {
                        anyhow::bail!("--select EPC must be exactly 24 hex characters");
                    }
                    Some(b)
                }
                None => None,
            };

            let bank_name = match bank {
                0 => "Reserved",
                1 => "EPC",
                2 => "TID",
                3 => "User",
                _ => "Unknown",
            };
            println!(
                "Writing {} bytes to bank {} ({}), addr 0x{:04X}...",
                data_bytes.len(),
                bank,
                bank_name,
                addr
            );

            println!("Place a tag on the antenna...");
            let current_epc = loop {
                let tags = connector.single_polling_instruction()?;
                if let Some(tag) = tags.first() {
                    break parse_hex(&tag.epc)?;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            let select_epc = select_bytes.as_ref().unwrap_or(&current_epc);
            println!("Selecting tag: {}", hex(select_epc));
            connector.select_tag(select_epc)?;
            connector.write_mem(&access_bytes, bank, addr, &data_bytes)?;
            connector.clear_select()?;
            println!("  Wrote: {}", hex(&data_bytes));
        }

        Commands::Kill { password, select } => {
            let pwd_bytes = parse_hex(&password)?;
            if pwd_bytes.len() != 4 {
                anyhow::bail!("Kill password must be exactly 8 hex characters (4 bytes)");
            }
            let select_bytes = match &select {
                Some(s) => {
                    let b = parse_hex(s)?;
                    if b.len() != 12 {
                        anyhow::bail!("--select EPC must be exactly 24 hex characters");
                    }
                    Some(b)
                }
                None => None,
            };

            println!("Place a tag on the antenna...");
            let current_epc = loop {
                let tags = connector.single_polling_instruction()?;
                if let Some(tag) = tags.first() {
                    break parse_hex(&tag.epc)?;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            let select_epc = select_bytes.as_ref().unwrap_or(&current_epc);
            println!("Selecting tag: {}", hex(select_epc));
            connector.select_tag(select_epc)?;
            println!("Killing tag with password {}...", hex(&pwd_bytes));
            connector.kill_tag(&pwd_bytes)?;
            connector.clear_select()?;
            println!("Tag killed.");
        }

        Commands::Lock {
            password,
            lock_data,
            select,
        } => {
            let pwd_bytes = parse_hex(&password)?;
            if pwd_bytes.len() != 4 {
                anyhow::bail!("Lock password must be exactly 8 hex characters (4 bytes)");
            }
            let ld_bytes = parse_hex(&lock_data)?;
            if ld_bytes.len() != 3 {
                anyhow::bail!("--lock-data must be exactly 6 hex characters (3 bytes)");
            }
            let select_bytes = match &select {
                Some(s) => {
                    let b = parse_hex(s)?;
                    if b.len() != 12 {
                        anyhow::bail!("--select EPC must be exactly 24 hex characters");
                    }
                    Some(b)
                }
                None => None,
            };

            println!("Place a tag on the antenna...");
            let current_epc = loop {
                let tags = connector.single_polling_instruction()?;
                if let Some(tag) = tags.first() {
                    break parse_hex(&tag.epc)?;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            let select_epc = select_bytes.as_ref().unwrap_or(&current_epc);
            println!("Selecting tag: {}", hex(select_epc));
            connector.select_tag(select_epc)?;
            println!(
                "Locking tag (password {}, lock data {})...",
                hex(&pwd_bytes),
                hex(&ld_bytes)
            );
            connector.lock_tag(&pwd_bytes, &ld_bytes)?;
            connector.clear_select()?;
            println!("Tag locked.");
        }

        Commands::Read {
            bank,
            addr,
            length,
            access,
            select,
        } => {
            let access_bytes = parse_hex(&access)?;
            if access_bytes.len() != 4 {
                anyhow::bail!("--access must be exactly 8 hex characters (4 bytes)");
            }
            let select_bytes = match &select {
                Some(s) => {
                    let b = parse_hex(s)?;
                    if b.len() != 12 {
                        anyhow::bail!("--select EPC must be exactly 24 hex characters");
                    }
                    Some(b)
                }
                None => None,
            };

            println!("Place a tag on the antenna...");
            let current_epc = loop {
                let tags = connector.single_polling_instruction()?;
                if let Some(tag) = tags.first() {
                    break parse_hex(&tag.epc)?;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            let select_epc = select_bytes.as_ref().unwrap_or(&current_epc);
            println!("Selecting tag: {}", hex(select_epc));
            connector.select_tag(select_epc)?;
            let bank_name = match bank {
                0 => "Reserved",
                1 => "EPC",
                2 => "TID",
                3 => "User",
                _ => "Unknown",
            };
            println!(
                "Reading bank {} ({}), addr 0x{:04X}, {} words...",
                bank, bank_name, addr, length
            );
            let data = connector.read_mem(&access_bytes, bank, addr, length)?;
            println!("  Data: {}", hex(&data));
            connector.clear_select()?;
        }

        Commands::Region { area } => {
            let area_enum = match area.as_str() {
                "china900" => WorkingArea::China900Mhz,
                "china800" => WorkingArea::China800Mhz,
                "eu" => WorkingArea::EU,
                "us" => WorkingArea::US,
                "korea" => WorkingArea::Korea,
                _ => unreachable!(),
            };
            connector.set_working_area(area_enum)?;
            println!("Region set to {:?}.", area_enum);
        }

        Commands::Power { level } => match level {
            Some(p) => {
                connector.set_transmission_power(p)?;
                println!("Power set to {} dBm.", p);
            }
            None => {
                let p = connector.get_transmit_power()?;
                println!("Power: {} dBm", p);
            }
        },
    }

    Ok(())
}

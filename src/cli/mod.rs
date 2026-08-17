use clap::Parser;

mod display;
mod port;

#[derive(Parser)]
#[command(name = "r200", about = "R200 UHF RFID reader CLI")]
pub struct Cli {
    #[arg(short, long, env = "R200_PORT")]
    pub port: String,

    #[arg(short, long, default_value_t = 115200)]
    pub baud: u32,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(clap::Subcommand)]
pub enum Commands {
    Info,
    Poll,
    Scan,
    Read {
        bank: String,
        address: String,
        length: String,
    },
    Write {
        epc: String,
    },
    Kill {
        password: String,
    },
    Lock {
        password: String,
        lock_data: String,
    },
    Region {
        set: Option<String>,
    },
    Power {
        set: Option<f64>,
    },
}

pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let port = serialport::new(&cli.port, cli.baud)
        .open_native()
        .map_err(|e| anyhow::anyhow!("failed to open {}: {}", cli.port, e))?;

    let mut reader = crate::sync::SyncReader::new(port);

    match cli.command {
        Commands::Info => {
            let hw = reader.send(&crate::GetModuleInfo {
                param: crate::ModuleInfoParam::HardwareVersion,
            })?;
            let sw = reader.send(&crate::GetModuleInfo {
                param: crate::ModuleInfoParam::SoftwareVersion,
            })?;
            let mfr = reader.send(&crate::GetModuleInfo {
                param: crate::ModuleInfoParam::Manufacturer,
            })?;
            let region = reader.send(&crate::GetWorkingArea)?;
            let channel = reader.send(&crate::GetWorkingChannel)?;
            let power = reader.send(&crate::GetTransmitPower)?;

            println!("Hardware:   {}", hw.text);
            println!("Firmware:   {}", sw.text);
            println!("Manufacturer: {}", mfr.text);
            println!("Region:     {region}");
            println!(
                "Channel:    {} ({:.2} MHz)",
                channel.current_channel, channel.frequency_mhz
            );
            println!("Power:      {power:.1} dBm");
        }
        Commands::Poll => {
            loop {
                match reader.send(&crate::SinglePollingInstruction)? {
                    Some(tag) => {
                        println!("{tag}");
                        break;
                    }
                    None => std::thread::sleep(std::time::Duration::from_millis(50)),
                }
            }
        }
        Commands::Scan => {
            println!("Scanning... (Ctrl+C to stop)");
            reader.send(&crate::MultiplePollingInstruction { duration_ms: 0 })?;
            let mut seen = std::collections::HashSet::new();
            loop {
                match reader.send(&crate::SinglePollingInstruction)? {
                    Some(tag) => {
                        if seen.insert(tag.epc_hex()) {
                            display::display_tag(&tag);
                        }
                    }
                    None => std::thread::sleep(std::time::Duration::from_millis(10)),
                }
            }
        }
        _ => {
            anyhow::bail!("command not yet implemented");
        }
    }

    Ok(())
}

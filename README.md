# r200_uhf — R200 UHF serial protocol (Rust)

## Overview
- A small Rust library to talk with R200 UHF RFID reader modules over a serial port.
- Exposes a simple `Connector` API (blocking `SyncIO` or async `AsyncIO`) to query
  device info, inventory tags, and read/write/kill/lock tag memory banks.

## Supported operations
- Inventory: single and multiple polling instruction, tag selection.
- Memory: read and write any Gen2 memory bank (Reserved, EPC, TID, User).
- Tag lifecycle: kill and lock a tag.
- Radio: working area (region), working channel, transmission power.

## Getting started
### Requirements
- Rust toolchain (stable)
- Access to a serial port where the [R200 UHF reader](https://www.aliexpress.com/item/4000281733851.html) is connected 

### Add dependency:
```toml
[dependencies]
r200_uhf = "0.5"
serialport = "4.8"
```

## Run the example (quick start)
This repo includes an example that opens a serial port, configures power, and continuously reads tags.

- Linux/macOS example:
  cargo run --example std_pc_serial -- /dev/ttyUSB0 115200

- Windows example (port name may vary):
  cargo run --example std_pc_serial -- COM3 115200

Notes
- The baud argument is optional and defaults to 115200 when omitted.
- The example prints module info, current working area/channel, transmission power, and then logs any detected tags.

Minimal usage example (library)

```rust
use r200_uhf::Connector;
use std::time::Duration;
use serialport;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open the serial port to the R200 module
    let port = serialport::new("/dev/ttyUSB0", 115200)
        .timeout(Duration::from_millis(500))
        .open()?;

    // Create the Connector
    let mut conn = Connector::new(port);

    // Query some information
    let _info = conn.get_module_info()?;

    // Read tags once
    let tags = conn.single_polling_instruction()?;
    for t in tags {
        println!("{}", t); // Rfid implements Display
        // Access UID as hex string: t.uid()
    }

    // Read 2 words from the reserved bank of the selected tag
    let words = conn.read_mem(&[0, 0, 0, 0], 0, 0, 2)?;

    // Write a new EPC (bank 1, word address 2)
    conn.write_epc(&[0xE0, 0x28, 0x06, 0x91, 0x05, 0x00])?;

    Ok(())
}
```

For non-blocking I/O enable the `async` feature:

```toml
r200_uhf = { version = "0.5", features = ["async"] }
```

## CLI (`r200`)

This crate ships an optional command-line interface, exposed as a `r200` binary.
It is gated behind the `cli` cargo feature (off by default) and talks to the
reader using the blocking `SyncIO` API.

### Install

Install the latest published release of the `r200` binary from crates.io:

```sh
cargo install r200_uhf --features cli
```

Or build and install it from this repository:

```sh
cargo install --path . --features cli
```

Either way, `r200` ends up in `~/.cargo/bin`. Alternatively, run it directly
without installing:

```sh
cargo run --features cli --bin r200 -- --port /dev/ttyUSB0 info
```

### Usage

The serial port is given with `--port` (or the `R200_PORT` environment
variable); it must come from one of the two. The baud rate defaults to 115200
and can be set with `--baud`.

Here the port is set once via the environment, so it can be omitted from every
command:

```sh
export R200_PORT=/dev/ttyUSB0

# Show module info and current region/power
r200 info

# Wait until a tag is detected and print it
r200 poll

# Continuously scan for tags until Ctrl+C (no duplicates)
r200 scan

# Read 6 words from the EPC bank (bank 1, addr 0)
r200 read --bank 1 --addr 0 --length 6

# Write a new 12-byte EPC (24 hex chars)
r200 write E28069150000501D63E2784F

# Set the RF region (china900, china800, eu, us, korea)
r200 region eu

# Get or set the transmit power in dBm
r200 power
r200 power 26.5
```

Run `r200 --help` or `r200 <command> --help` for the full list of commands and
options.

Legal and safety note
- Transmission power and permitted frequencies vary by country/region. Ensure compliance with your local regulations. The example sets or checks transmission power; adjust it responsibly.

License
- MIT License. See LICENSE for details.

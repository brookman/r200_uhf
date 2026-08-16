use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};

use serialport::SerialPort;

use crate::frame::{Command, Frame};

/// A serial port shared between the connector and the reader guard.
pub struct SharedPort {
    inner: Arc<Mutex<Box<dyn SerialPort>>>,
    needs_flush: std::sync::atomic::AtomicBool,
}

impl SharedPort {
    pub fn new(port: Box<dyn SerialPort>) -> Self {
        SharedPort {
            inner: Arc::new(Mutex::new(port)),
            needs_flush: std::sync::atomic::AtomicBool::new(true),
        }
    }

    pub fn clear_input(&self) -> anyhow::Result<()> {
        self.inner
            .lock()
            .unwrap()
            .clear(serialport::ClearBuffer::Input)?;
        Ok(())
    }

    fn send_raw(&self, frame: &[u8]) -> io::Result<()> {
        let mut port = self.inner.lock().unwrap();
        port.write_all(frame)?;
        port.flush()?;
        Ok(())
    }
}

impl Clone for SharedPort {
    fn clone(&self) -> Self {
        SharedPort {
            inner: Arc::clone(&self.inner),
            needs_flush: std::sync::atomic::AtomicBool::new(true),
        }
    }
}

impl Read for SharedPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.needs_flush
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.inner.lock().unwrap().read(buf)
    }
}

impl Write for SharedPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self
            .needs_flush
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.inner
                .lock()
                .unwrap()
                .clear(serialport::ClearBuffer::Input)?;
        }
        self.inner.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().unwrap().flush()
    }
}

/// Stops multi-polling on the shared port when dropped.
pub struct ReaderGuard {
    port: SharedPort,
}

impl ReaderGuard {
    pub fn new(port: SharedPort) -> Self {
        ReaderGuard { port }
    }
}

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        let frame = Frame::new(&Command::StopMultiplePollingInstruction).to_bytes();
        self.port.send_raw(&frame).ok();
    }
}

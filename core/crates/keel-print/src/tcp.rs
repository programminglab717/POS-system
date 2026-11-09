//! Raw TCP to port 9100, one connection per job, on `std::net`.

use core::time::Duration;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};

use crate::{NotSent, Outcome, Sent, Transport, Waits};
use keel_doc::Confirm;

/// The port network printers take raw jobs on.
pub const PORT: u16 = 9100;

/// A connection to a printer.
pub struct Tcp {
    stream: TcpStream,
}

impl Tcp {
    /// Connects to `address`, waiting at most `wait`.
    ///
    /// # Errors
    /// Any error connecting.
    pub fn connect(address: SocketAddr, wait: Duration) -> io::Result<Tcp> {
        let stream = TcpStream::connect_timeout(&address, wait)?;
        stream.set_nodelay(true)?;
        Ok(Tcp { stream })
    }

    /// Waits at most `wait` for a write to make progress: a printer that stops reading, its
    /// buffer full or its cover opened mid-job, fails the send rather than holding it for ever.
    ///
    /// # Errors
    /// Any error setting it.
    pub fn set_send_wait(&mut self, wait: Duration) -> io::Result<()> {
        self.stream.set_write_timeout(Some(wait.max(Duration::from_millis(1))))
    }
}

impl Transport for Tcp {
    fn send(&mut self, bytes: &[u8]) -> Result<(), Sent> {
        let mut accepted = 0_usize;
        while let Some(rest) = bytes.get(accepted..).filter(|rest| !rest.is_empty()) {
            match self.stream.write(rest) {
                Ok(0) => {
                    return Err(Sent { accepted, error: io::ErrorKind::WriteZero.into() });
                }
                Ok(n) => accepted = accepted.saturating_add(n),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(Sent { accepted, error }),
            }
        }
        self.stream.flush().map_err(|error| Sent { accepted, error })
    }

    fn receive(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
        // A zero timeout would mean waiting for ever.
        self.stream.set_read_timeout(Some(wait.max(Duration::from_millis(1))))?;
        loop {
            match self.stream.read(buf) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                other => return other,
            }
        }
    }

    fn finish(&mut self) -> io::Result<()> {
        self.stream.shutdown(Shutdown::Write)
    }
}

/// Runs `job` against the printer at `address`, as [`crate::print`] does, connecting first: a
/// printer that can't be reached within the status wait gets none of it.
pub fn print(
    address: SocketAddr,
    job: &[u8],
    confirm: Confirm,
    id: [u8; 4],
    waits: Waits,
) -> Outcome {
    match Tcp::connect(address, waits.status) {
        Ok(mut tcp) => match tcp.set_send_wait(waits.word) {
            Ok(()) => crate::print(&mut tcp, job, confirm, id, waits),
            Err(_) => Outcome::NotSent(NotSent::Unreachable),
        },
        Err(_) => Outcome::NotSent(NotSent::Unreachable),
    }
}

//! Bind the first free port at or above the one asked for.

use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs as _};

use socket2::{Domain, Protocol, Socket, Type};

const PORT_SCAN_ATTEMPTS: u32 = 20;
const BACKLOG: i32 = 128;

/// Why no port could be bound; `Display` is the message for the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindError {
    #[error("Permission denied binding port {0} (privileged port; try a port >= 1024).")]
    Denied(u16),
    #[error("Failed to bind port {port}: {message}")]
    Failed { port: u16, message: String },
    #[error("No free port in range {start}..{end}; pass --port to choose another.")]
    Exhausted { start: u16, end: u32 },
}

fn bind_one(host: &str, port: u16) -> std::io::Result<TcpListener> {
    let addr = (host, port).to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(
            ErrorKind::AddrNotAvailable,
            format!("cannot resolve {host}"),
        )
    })?;
    if addr.is_ipv6() && addr.ip().is_unspecified() {
        bind_dual_stack(addr)
    } else {
        TcpListener::bind(addr)
    }
}

/// `::` that also accepts IPv4, as Bun does. Windows and Linux with
/// `bindv6only=1` default to IPv6 only, which would refuse the
/// `127.0.0.1` URL the banner prints.
fn bind_dual_stack(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_only_v6(false)?;
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(BACKLOG)?;
    Ok(socket.into())
}

/// Try `start..start+20` on `host` and keep the first listener that binds.
pub fn bind_free_port(host: &str, start: u16) -> Result<(TcpListener, u16), BindError> {
    let end = u32::from(start) + PORT_SCAN_ATTEMPTS;
    for port in (u32::from(start)..end).filter_map(|p| u16::try_from(p).ok()) {
        match bind_one(host, port) {
            Ok(l) => return Ok((l, port)),
            Err(e) if e.kind() == ErrorKind::AddrInUse => {}
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                return Err(BindError::Denied(port));
            }
            Err(e) => {
                return Err(BindError::Failed {
                    port,
                    message: e.to_string(),
                });
            }
        }
    }
    Err(BindError::Exhausted { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_port_moves_to_the_next() {
        let (first, port) = bind_free_port("127.0.0.1", 47_310).unwrap();
        let (_second, next) = bind_free_port("127.0.0.1", port).unwrap();
        assert!(next > port);
        drop(first);
    }
    #[test]
    fn ipv6_wildcard_accepts_ipv4() {
        let Ok((listener, port)) = bind_free_port("::", 47_420) else {
            return; // host without IPv6
        };
        assert!(!socket2::SockRef::from(&listener).only_v6().unwrap());
        let client = std::net::TcpStream::connect(("127.0.0.1", port));
        assert!(client.is_ok(), "{client:?}");
        let (_peer, from) = listener.accept().unwrap();
        assert!(from.ip().to_canonical().is_loopback(), "{from}");
    }
    // find-free-port.test.ts: a client polling the port does not stall the scan.
    #[test]
    fn a_client_connecting_during_the_scan_does_not_stall_it() {
        let (first, port) = bind_free_port("127.0.0.1", 44_200).unwrap();
        drop(first);
        let polling = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let flag = std::sync::Arc::clone(&polling);
        let poller = std::thread::spawn(move || {
            let mut sockets = Vec::new();
            while flag.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                    sockets.push(s);
                }
                std::thread::yield_now();
            }
        });
        let started = std::time::Instant::now();
        for _ in 0..200 {
            let (listener, got) = bind_free_port("127.0.0.1", port).unwrap();
            assert!(got >= port);
            drop(listener);
        }
        let elapsed = started.elapsed();
        polling.store(false, std::sync::atomic::Ordering::Relaxed);
        poller.join().unwrap();
        assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    }
}

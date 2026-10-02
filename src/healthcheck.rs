//! `rust-services healthcheck` – container health probe without curl (step 024).
//!
//! Distroless / scratch images contain no shell and no curl, so `HEALTHCHECK CMD curl ...` is
//! impossible. The binary checks itself: a minimal synchronous HTTP request to the liveness
//! endpoint. Exit code 0 = healthy, 1 = unhealthy.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

pub fn run() -> std::process::ExitCode {
    // Same variable the server uses; the probe runs inside the container -> localhost.
    let port = std::env::var("APP_SERVER__PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3000u16);

    match probe(SocketAddr::from(([127, 0, 0, 1], port))) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("healthcheck failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn probe(address: SocketAddr) -> std::io::Result<()> {
    let timeout = Duration::from_secs(2);
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream
        .write_all(b"GET /health/live HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;

    // Only the status line matters: "HTTP/1.1 200 OK".
    let mut buffer = [0u8; 32];
    let read = stream.read(&mut buffer)?;
    let status_line = String::from_utf8_lossy(&buffer[..read]);
    if status_line.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "unexpected response: {}",
            status_line.lines().next().unwrap_or_default()
        )))
    }
}

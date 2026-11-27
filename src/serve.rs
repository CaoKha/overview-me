//! Serve one generated page on the machine's Tailscale address.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

/// The URL path travels in the environment, not argv, so `ps` does not show the token.
const PATH_VAR: &str = "OVERVIEW_ME_SERVE_PATH";

/// Binds the tailnet address, then hands the socket to a detached `serve-detached` child
/// (as its stdin) so this command returns at once. Returns the page URL.
pub(crate) fn start(html: &Path) -> Result<String> {
    let ip = tailscale_ip()?;
    let listener = TcpListener::bind((ip, 0)).with_context(|| format!("binding to {ip}"))?;
    let addr = listener.local_addr()?;
    let mut random = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut urandom| urandom.read_exact(&mut random))
        .context("generating the page URL token")?;
    let path = format!("/{:032x}/", u128::from_be_bytes(random));
    Command::new(std::env::current_exe()?)
        .arg("serve-detached")
        .arg("--")
        .arg(html)
        .env(PATH_VAR, &path)
        .process_group(0)
        .stdin(OwnedFd::from(listener))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting the page server")?;
    Ok(format!("http://{addr}{path}"))
}

/// The detached child: serves `html` on the listener it got as stdin for 30 minutes.
pub(crate) fn run(html: &Path) -> Result<()> {
    let path = std::env::var(PATH_VAR).with_context(|| format!("{PATH_VAR} is not set"))?;
    let listener = TcpListener::from(io::stdin().as_fd().try_clone_to_owned()?);
    // Expire even when accept or a client connection is blocking.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(30 * 60));
        std::process::exit(0);
    });
    // A thread per connection: browsers pre-open idle sockets that would hold up the real request.
    thread::scope(|scope| {
        while let Ok((stream, peer)) = listener.accept() {
            // Tailnet peers only: a LAN client routed in through a subnet router has a LAN address.
            if matches!(peer.ip(), IpAddr::V4(ip) if in_tailnet_range(ip)) {
                scope.spawn(|| handle(stream, &path, html));
            }
        }
    });
    Ok(())
}

fn tailscale_ip() -> Result<Ipv4Addr> {
    Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let first = output.stdout.split(|&byte| byte == b'\n').next()?;
            std::str::from_utf8(first).ok()?.trim().parse().ok()
        })
        .filter(|&ip| in_tailnet_range(ip))
        .context("no Tailscale address: install Tailscale and connect this machine to your tailnet (tailscale ip -4)")
}

fn in_tailnet_range(ip: Ipv4Addr) -> bool {
    let [first, second, ..] = ip.octets();
    first == 100 && (64..128).contains(&second)
}

fn handle(mut stream: TcpStream, path: &str, html: &Path) -> io::Result<()> {
    let timeout = Some(Duration::from_secs(5));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new((&mut stream).take(8 * 1024));
    let mut line = Vec::new();
    let mut read = reader.read_until(b'\n', &mut line);
    let requested = line
        .strip_suffix(b" HTTP/1.1\r\n")
        .or_else(|| line.strip_suffix(b" HTTP/1.0\r\n"))
        .and_then(|target| target.strip_prefix(b"GET "))
        == Some(path.as_bytes());
    // Drain the headers: closing with unread bytes sends a reset, which can drop the response.
    while matches!(read, Ok(1..)) && line != b"\r\n" && line != b"\n" {
        line.clear();
        read = reader.read_until(b'\n', &mut line);
    }
    let Some(body) = requested.then(|| fs::read(html).ok()).flatten() else {
        return stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        );
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Cache-Control: no-store\r\n\
         Referrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\
         Content-Length: {}\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempRepo;

    #[test]
    fn tailscale_ipv4_range() {
        for ip in ["100.64.0.1", "100.127.255.254"] {
            assert!(in_tailnet_range(ip.parse().expect("valid IPv4")), "{ip}");
        }
        for ip in ["100.63.255.255", "100.128.0.0", "192.168.1.2"] {
            assert!(!in_tailnet_range(ip.parse().expect("valid IPv4")), "{ip}");
        }
    }

    #[test]
    fn serves_only_the_tokenized_get() {
        let repo = TempRepo::new("serve");
        repo.write("page.html", "<h1>Overview</h1>");
        let html = repo.path().join("page.html");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("test address");
        let request = |line: &str| {
            let mut client = TcpStream::connect(addr).expect("connect to test listener");
            write!(client, "{line}\r\nHost: test\r\n\r\n").expect("write request");
            let (server, _) = listener.accept().expect("accept test connection");
            handle(server, "/good/", &html).expect("handle request");
            let mut response = String::new();
            client.read_to_string(&mut response).expect("read response");
            response
        };
        let response = request("GET /good/ HTTP/1.1");
        let (headers, body) = response.split_once("\r\n\r\n").expect("HTTP response");
        assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(headers.contains("\r\nReferrer-Policy: no-referrer\r\n"));
        assert_eq!(body, "<h1>Overview</h1>");

        for line in [
            "GET /wrong/ HTTP/1.1",
            "GET / HTTP/1.1",
            "POST /good/ HTTP/1.1",
            "HEAD /good/ HTTP/1.1",
            "GET /good/?x=1 HTTP/1.1",
            "GET /good/",
        ] {
            assert_eq!(
                request(line),
                "HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
                "{line}"
            );
        }
    }
}

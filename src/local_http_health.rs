//! Bounded local liveness probe. Reads one complete HTTP status line, never a body/EOF.
use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    time::{Duration, Instant},
};
const MAX_STATUS_LINE: usize = 512;

pub(crate) fn probe_path(prefix: &str) -> String {
    let prefix = prefix.trim().trim_matches('/');
    if prefix.is_empty() {
        "/config.js".into()
    } else {
        format!("/{prefix}/config.js")
    }
}

fn status_ready(line: &[u8]) -> bool {
    let Some(line) = line.strip_suffix(b"\r\n") else {
        return false;
    };
    let Ok(line) = std::str::from_utf8(line) else {
        return false;
    };
    let mut fields = line.splitn(3, ' ');
    if !matches!(fields.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return false;
    }
    let Some(code) = fields.next() else {
        return false;
    };
    code.len() == 3
        && code.bytes().all(|b| b.is_ascii_digit())
        && code.parse::<u16>().is_ok_and(|c| (200..500).contains(&c))
}

fn read_status(stream: &mut TcpStream, budget: Duration) -> bool {
    let started = Instant::now();
    let mut line = Vec::with_capacity(128);
    let mut chunk = [0_u8; 128];
    while line.len() < MAX_STATUS_LINE {
        let Some(remaining) = budget
            .checked_sub(started.elapsed())
            .filter(|v| !v.is_zero())
        else {
            return false;
        };
        if stream.set_read_timeout(Some(remaining)).is_err() {
            return false;
        }
        let size = chunk.len().min(MAX_STATUS_LINE - line.len());
        let count = match stream.read(&mut chunk[..size]) {
            Ok(n) if n > 0 => n,
            _ => return false,
        };
        if let Some(end) = chunk[..count].iter().position(|b| *b == b'\n') {
            line.extend_from_slice(&chunk[..=end]);
            return status_ready(&line);
        }
        line.extend_from_slice(&chunk[..count]);
    }
    false
}

pub(crate) fn probe(mut address: SocketAddr, prefix: &str) -> bool {
    if address.ip().is_unspecified() {
        address.set_ip(if address.is_ipv4() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        });
    }
    let path = probe_path(prefix);
    if path.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return false;
    }
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_secs(2)) else {
        return false;
    };
    if stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .is_err()
    {
        return false;
    }
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    read_status(&mut stream, Duration::from_secs(3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, sync::mpsc, thread};
    #[test]
    fn validates_complete_status_not_arbitrary_or_partial_bytes() {
        for line in [
            b"HTTP/1.1 200 OK\r\n".as_slice(),
            b"HTTP/1.0 401 Unauthorized\r\n",
            b"HTTP/1.1 404 Not Found\r\n",
        ] {
            assert!(status_ready(line));
        }
        for line in [
            b"junk 200 OK\r\n".as_slice(),
            b"HTTP/1.1 200",
            b"HTTP/1.1 2000 OK\r\n",
            b"HTTP/1.1 500 Error\r\n",
            b"HTTP/1.1 100 Continue\r\n",
            b"HTTP/1.1 200 OK\n",
            b"",
        ] {
            assert!(!status_ready(line));
        }
    }
    #[test]
    fn path_targets_config_not_index() {
        assert_eq!(probe_path(" /stream/ "), "/stream/config.js");
        assert_eq!(probe_path("stream"), "/stream/config.js");
        assert_eq!(probe_path(""), "/config.js");
    }
    #[test]
    fn fragmented_status_succeeds_without_body_or_connection_close() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (done, wait) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 1024];
            let n = client.read(&mut request).unwrap();
            assert!(
                String::from_utf8_lossy(&request[..n])
                    .starts_with("GET /stream/config.js HTTP/1.1\r\n")
            );
            for fragment in [b"HT".as_slice(), b"TP/1.", b"1 ", b"20", b"0 OK\r", b"\n"] {
                client.write_all(fragment).unwrap();
                thread::sleep(Duration::from_millis(5));
            }
            // Probe must finish even while this peer refuses to send the body/EOF.
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
        });
        assert!(probe(address, "/stream"));
        done.send(()).unwrap();
        server.join().unwrap();
    }
    #[test]
    fn stalled_status_has_a_total_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server.write_all(b"HTTP/1.1 20").unwrap();
        let started = Instant::now();
        assert!(!read_status(&mut client, Duration::from_millis(70)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn oversize_and_eof_before_newline_fail_closed() {
        for body in [vec![b'x'; MAX_STATUS_LINE], b"HTTP/1.1 200 OK".to_vec()] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (mut server, _) = listener.accept().unwrap();
            server.write_all(&body).unwrap();
            drop(server);
            assert!(!read_status(&mut client, Duration::from_secs(1)));
        }
    }
}

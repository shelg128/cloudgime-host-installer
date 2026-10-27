//! Bounded HTTPS status transport for Windows TLS/proxy interoperability.
//! Credentials travel only over child stdin; never argv, temp files or logs.
use serde_json::Value;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
const LIMIT: usize = 65_536;
const STATUS_URL: &str = "https://cloudgime.my.id/api/v1/host-activation/status";
fn parse_output(bytes: &[u8], exit_ok: bool) -> Result<(u16, Value), &'static str> {
    if !exit_ok {
        return Err("NODE_STATUS_CURL_TRANSPORT_FAILED");
    }
    if bytes.len() > LIMIT {
        return Err("NODE_STATUS_CURL_RESPONSE_TOO_LARGE");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "NODE_STATUS_CURL_RESPONSE_INVALID")?;
    let (body, code) = text
        .trim_end()
        .rsplit_once('\n')
        .ok_or("NODE_STATUS_CURL_RESPONSE_INVALID")?;
    let code: u16 = code
        .trim()
        .parse()
        .map_err(|_| "NODE_STATUS_CURL_RESPONSE_INVALID")?;
    if !(100..=599).contains(&code) {
        return Err("NODE_STATUS_CURL_RESPONSE_INVALID");
    }
    if !(200..300).contains(&code) {
        return Ok((code, Value::Null));
    }
    let payload = serde_json::from_str(body).map_err(|_| "NODE_STATUS_CURL_RESPONSE_INVALID")?;
    Ok((code, payload))
}
fn read_bounded(mut stream: impl Read) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let keep = n.min((LIMIT + 1).saturating_sub(data.len()));
        data.extend_from_slice(&buf[..keep]);
    }
    Ok(data)
}
#[cfg(windows)]
pub(super) fn status(body: &Value) -> Result<(u16, Value), &'static str> {
    use std::os::windows::process::CommandExt;
    // System-supplied curl, not a PATH executable that a renter could substitute.
    let exe = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").ok_or("NODE_STATUS_CURL_SYSTEMROOT_MISSING")?,
    )
    .join("System32")
    .join("curl.exe");
    let mut input = serde_json::to_vec(body).map_err(|_| "NODE_STATUS_REQUEST_INVALID")?;
    if input.len() > 8192 {
        input.fill(0);
        return Err("NODE_STATUS_REQUEST_TOO_LARGE");
    }
    let mut child = Command::new(exe)
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "0",
            "--connect-timeout",
            "3",
            "--max-time",
            "7",
            "--max-filesize",
            "65536",
            "--request",
            "POST",
            "--header",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            "--write-out",
            "\n%{http_code}",
            STATUS_URL,
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "NODE_STATUS_CURL_UNAVAILABLE")?;
    let out = child.stdout.take().ok_or("NODE_STATUS_CURL_PIPE")?;
    let err = child.stderr.take().ok_or("NODE_STATUS_CURL_PIPE")?;
    let out_thread = thread::spawn(move || read_bounded(out));
    let err_thread = thread::spawn(move || read_bounded(err));
    let write = child
        .stdin
        .take()
        .ok_or("NODE_STATUS_CURL_PIPE")?
        .write_all(&input);
    input.fill(0);
    if write.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        let _ = out_thread.join();
        let _ = err_thread.join();
        return Err("NODE_STATUS_CURL_INPUT_FAILED");
    }
    let began = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s.success()),
            Ok(None) if began.elapsed() < Duration::from_secs(9) => {
                thread::sleep(Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("NODE_STATUS_CURL_TIMEOUT");
            }
        }
    };
    let output = out_thread
        .join()
        .map_err(|_| "NODE_STATUS_CURL_READ")?
        .map_err(|_| "NODE_STATUS_CURL_READ")?;
    // Never propagate stderr: it can include URL/request details from TLS tooling.
    let _ = err_thread.join();
    parse_output(&output, result?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_success_json_and_status() {
        assert_eq!(
            parse_output(b"{\"ok\":true}\n200", true).unwrap(),
            (200, serde_json::json!({"ok":true}))
        );
    }
    #[test]
    fn transport_error_cannot_become_success() {
        assert!(parse_output(b"{\"ok\":true}\n200", false).is_err());
    }
    #[test]
    fn unauthorized_and_redirect_are_explicit_status_not_success() {
        for code in [301, 401, 403, 409, 500] {
            let x = format!("not-json\n{code}");
            assert_eq!(
                parse_output(x.as_bytes(), true).unwrap(),
                (code, Value::Null)
            );
        }
    }
    #[test]
    fn malformed_oversize_and_zero_status_rejected() {
        for bytes in [
            &b"{}"[..],
            &b"garbage\n200"[..],
            &b"{}\n000"[..],
            &b"{}\n999"[..],
        ] {
            assert!(parse_output(bytes, true).is_err());
        }
        assert!(parse_output(&vec![b'x'; LIMIT + 1], true).is_err());
    }
    #[test]
    fn bounded_reader_never_retains_unbounded_output() {
        assert_eq!(
            read_bounded(std::io::Cursor::new(vec![b'x'; LIMIT * 4]))
                .unwrap()
                .len(),
            LIMIT + 1
        );
    }
    #[test]
    fn request_contract_disallows_secret_argv_files_redirects() {
        let s = include_str!("host_status_transport.rs");
        let implementation = s.split("#[cfg(test)]").next().unwrap();
        assert!(implementation.contains("\"--disable\""));
        assert!(implementation.contains("\"@-\""));
        assert!(implementation.contains("\"--max-redirs\",\"0\""));
        assert!(!implementation.contains("--insecure"));
        assert!(!implementation.contains("fs::write"));
        assert!(implementation.contains("input.fill(0)"));
    }
}

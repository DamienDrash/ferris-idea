//! Integration tests of `ferris-idea-sandbox` (`SPEC-0347`) as one test program. The enforcement
//! tests start this program itself inside the sandbox as the child that tries to write, read and
//! connect ([`sandbox_child`]).

mod proxy;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod child;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

/// The child's side: with `FERRIS_IDEA_SANDBOX_CHILD` set it runs the actions named there and prints
/// one `RESULT <action> ok` or `RESULT <action> denied <error>` line each; without it, nothing.
#[test]
fn sandbox_child() {
    let Some(actions) = std::env::var_os("FERRIS_IDEA_SANDBOX_CHILD") else {
        return;
    };
    for action in actions.to_string_lossy().split(';') {
        let outcome = run(action);
        match outcome {
            Ok(()) => println!("RESULT {action} ok"),
            Err(e) => println!("RESULT {action} denied {e}"),
        }
    }
}

fn run(action: &str) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::time::Duration;
    let (kind, arg) = action.split_once(':').ok_or("bad action")?;
    match kind {
        "write" => std::fs::write(arg, "x").map_err(|e| e.to_string()),
        "read" => std::fs::read(arg).map(|_| ()).map_err(|e| e.to_string()),
        "connect" => {
            let addr: std::net::SocketAddr = arg.parse().map_err(|_| "bad address")?;
            std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3))
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        // `proxy:<host:port>`: CONNECT through HTTPS_PROXY; ok only for `200`.
        "proxy" => {
            let url = std::env::var("HTTPS_PROXY").map_err(|_| "no HTTPS_PROXY")?;
            let addr = url.trim_start_matches("http://");
            let mut s = std::net::TcpStream::connect(addr).map_err(|e| e.to_string())?;
            s.set_read_timeout(Some(Duration::from_secs(5))).ok();
            write!(s, "CONNECT {arg} HTTP/1.1\r\nHost: {arg}\r\n\r\n")
                .map_err(|e| e.to_string())?;
            let mut buf = [0u8; 256];
            let n = s.read(&mut buf).map_err(|e| e.to_string())?;
            let line = String::from_utf8_lossy(&buf[..n])
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            if line.contains(" 200 ") {
                Ok(())
            } else {
                Err(line)
            }
        }
        _ => Err("unknown action".into()),
    }
}

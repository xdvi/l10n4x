use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn get_workspace_root() -> std::path::PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn send_request(port: u16, path: &str, headers: &[&str]) -> Result<(u16, String), std::io::Error> {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;

    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n",
        path, port
    );
    for h in headers {
        request.push_str(h);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;

    let resp_str = String::from_utf8_lossy(&response).into_owned();

    let first_line = resp_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 2 {
        if let Ok(code) = parts[1].parse::<u16>() {
            return Ok((code, resp_str));
        }
    }

    Ok((500, resp_str))
}

struct KillOnDrop(Option<Child>);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn test_dev_server_token_auth() {
    let workspace_root = get_workspace_root();
    let port = 13456;

    let child = Command::new("cargo")
        .args([
            "run",
            "-p",
            "l10n4x-toolkit",
            "--",
            "dev",
            "--port",
            &port.to_string(),
        ])
        .current_dir(&workspace_root)
        .env("L10N4X_DEV_TOKEN", "my-test-token-123")
        .env("L10N4X_SIGNING_KEY", "0123456789abcdef0123456789abcdef")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start cargo run for dev server");

    let mut guard = KillOnDrop(Some(child));

    let start = Instant::now();
    let mut ready = false;
    let mut last_err = None;
    while start.elapsed() < Duration::from_secs(8) {
        match send_request(port, "/", &[]) {
            Ok((code, _)) => {
                if code == 401 || code == 404 {
                    ready = true;
                    break;
                } else {
                    last_err = Some(format!("HTTP code: {}", code));
                }
            }
            Err(e) => {
                last_err = Some(format!("Connection error: {}", e));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    if !ready {
        let mut child = guard.0.take().unwrap();
        let _ = child.kill();
        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        if let Some(mut out) = child.stdout.take() {
            let _ = out.read_to_string(&mut stdout_buf);
        }
        if let Some(mut err) = child.stderr.take() {
            let _ = err.read_to_string(&mut stderr_buf);
        }
        let _ = child.wait();
        panic!(
            "Dev server failed to start or respond within 8 seconds. Last polling diagnostic: {:?}\nSTDOUT:\n{}\nSTDERR:\n{}",
            last_err, stdout_buf, stderr_buf
        );
    }

    // 1. Assert protected endpoint without token returns 401
    let (code, _) =
        send_request(port, "/locales/nonexistent.pak", &[]).expect("Failed to query dev server");
    assert_eq!(code, 401, "Expected 401 Unauthorized without token");

    // 2. Assert protected endpoint with invalid token returns 401
    let (code, _) = send_request(
        port,
        "/locales/nonexistent.pak",
        &["Authorization: Bearer wrong-token"],
    )
    .expect("Failed to query dev server");
    assert_eq!(code, 401, "Expected 401 Unauthorized with wrong token");

    // 3. Assert protected endpoint with valid Bearer token returns 404 (file not found)
    let (code, _) = send_request(
        port,
        "/locales/nonexistent.pak",
        &["Authorization: Bearer my-test-token-123"],
    )
    .expect("Failed to query dev server");
    assert_eq!(code, 404, "Expected 404 Not Found with valid Bearer token");

    // 4. Assert protected endpoint with valid query parameter token returns 404 (file not found)
    let (code, _) = send_request(
        port,
        "/locales/nonexistent.pak?token=my-test-token-123",
        &[],
    )
    .expect("Failed to query dev server");
    assert_eq!(code, 404, "Expected 404 Not Found with valid query token");
}

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static DEV_SERVER_LOCK: Mutex<()> = Mutex::new(());

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
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

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

fn wait_for_server(port: u16, stderr: &mut impl Read) -> Result<(), String> {
    let start = Instant::now();
    let mut attempt: u32 = 0;
    let mut last_err = None;
    while start.elapsed() < Duration::from_secs(15) {
        match send_request(port, "/", &[]) {
            Ok((code, _)) if code == 401 || code == 404 => return Ok(()),
            Ok((code, _)) => last_err = Some(format!("unexpected HTTP code: {code}")),
            Err(e) => last_err = Some(format!("connection error: {e}")),
        }
        let backoff_ms = 50u64.saturating_mul(1u64 << attempt.min(6));
        std::thread::sleep(Duration::from_millis(backoff_ms));
        attempt = attempt.saturating_add(1);
    }
    let mut stderr_buf = String::new();
    let _ = stderr.read_to_string(&mut stderr_buf);
    Err(format!(
        "dev server not ready within 15s; last error: {:?}\nSTDERR:\n{}",
        last_err, stderr_buf
    ))
}

#[test]
fn test_dev_server_token_auth() {
    let _lock = DEV_SERVER_LOCK.lock().unwrap();
    let workspace_root = get_workspace_root();

    let dev_server_bin = env!("CARGO_BIN_EXE_l10n4x");
    let mut child = Command::new(dev_server_bin)
        .args(["dev", "--port", "0"])
        .current_dir(&workspace_root)
        .env("L10N4X_DEV_TOKEN", "my-test-token-123")
        .env("L10N4X_SIGNING_KEY", "0123456789abcdef0123456789abcdef")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start dev server binary");

    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(&mut stdout);
    let mut line = String::new();

    let port = loop {
        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            let _ = child.kill();
            let mut stderr_buf = String::new();
            let _ = stderr.read_to_string(&mut stderr_buf);
            let _ = child.wait();
            panic!(
                "Dev server exited before printing port.\nSTDERR:\n{}",
                stderr_buf
            );
        }
        if let Some(pos) = line.find("http://localhost:") {
            let port_part = &line[pos + 17..];
            let port_str: String = port_part
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            break port_str
                .parse::<u16>()
                .expect("Failed to parse port from stdout");
        }
    };

    let mut guard = KillOnDrop(Some(child));

    if let Err(msg) = wait_for_server(port, &mut stderr) {
        let mut child = guard.0.take().unwrap();
        let _ = child.kill();
        let _ = child.wait();
        panic!("{}", msg);
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

//! Launch the real GUI-subsystem hook binary through the Windows hook shell.
#![cfg(windows)]
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    time::Duration,
};

#[test]
fn windows_hook_shell_preserves_paths_input_and_approval_decisions() {
    use std::os::windows::process::CommandExt;
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("Zeren O'Brien 日本語");
    std::fs::create_dir(&folder).unwrap();
    let exe = folder.join("zeren.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_zeren"), &exe).unwrap();
    let hook = zeren_harness::agy::permission_hook_command(&exe).unwrap();
    assert!(hook.starts_with("powershell.exe "));
    assert!(!hook.contains('"'));
    for decision in ["deny", "auto_approve"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline, "Hook never connected");
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&mut socket).read_line(&mut line).unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["token"], "shell-test");
            assert_eq!(
                request["payload"]["toolCall"]["args"]["CommandLine"],
                "echo shell-test"
            );
            writeln!(socket, "{}", serde_json::json!({"decision":decision})).unwrap();
        });
        let mut child = Command::new("cmd.exe")
            .args(["/d", "/s", "/c", &hook])
            .env("ZEREN_AGY_APPROVAL_ADDRESS", address.to_string())
            .env("ZEREN_AGY_APPROVAL_TOKEN", "shell-test")
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                br#"{"toolCall":{"name":"run_command","args":{"CommandLine":"echo shell-test"}}}"#,
            )
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut body = String::new();
            BufReader::new(stdout).read_to_string(&mut body).unwrap();
            body
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(40);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                panic!("Hook shell timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(status.success());
        server.join().unwrap();
        let response: serde_json::Value = serde_json::from_str(&reader.join().unwrap()).unwrap();
        assert_eq!(response["decision"], decision);
    }
}

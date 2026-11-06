//! The background engine binary end to end: it serves its port at once, says
//! it posts the desktop banners, takes viewport focus, and stops cleanly when
//! asked — what the app relies on to start it, attach, and replace it.

use std::process::Command;
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn background_engine_serves_its_port_and_stops_on_request() {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_zeren-engine"))
        .env("ZEREN_DATA_DIR", dir.path())
        .env("ZEREN_IPC_PORT", port.to_string())
        // Development auth with no bearer: an offline runtime.
        .env("ZEREN_WORKOS_CLIENT_ID", "")
        .env_remove("ZEREN_EDGE_TOKEN")
        .env("ZEREN_EDGE_URL", "http://127.0.0.1:1")
        .env("ZEREN_DISABLE_NOTIFICATIONS", "1")
        .env("ZEREN_AUTO_UPDATE", "0")
        .spawn()
        .expect("start the background engine");

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let served = runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(60);
        let client = loop {
            match zeren_rpc::connect_ipc(port, Some(dir.path())).await {
                Ok(client) => break client,
                Err(err) if Instant::now() >= deadline => {
                    return Err(format!("never served its port: {err}"));
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        };
        let info = client
            .call(zeren_rpc::methods::ENGINE_INFO, serde_json::json!({}))
            .await
            .map_err(|err| format!("EngineInfo: {err}"))?;
        let capabilities: Vec<&str> = info["capabilities"]
            .as_array()
            .map(|list| list.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if !capabilities.contains(&"desktop-notifications-v1") {
            return Err(format!("no banner capability in {capabilities:?}"));
        }
        client
            .call(zeren_rpc::methods::ENGINE_READY, serde_json::json!({}))
            .await
            .map_err(|err| format!("EngineReady: {err}"))?;
        client
            .call(
                zeren_rpc::methods::SET_VIEWPORT_FOCUS,
                serde_json::json!({ "viewportId": "test", "focused": true }),
            )
            .await
            .map_err(|err| format!("SetViewportFocus: {err}"))?;
        client
            .call(zeren_rpc::methods::STOP_ENGINE, serde_json::json!({}))
            .await
            .map_err(|err| format!("StopEngine: {err}"))?;
        Ok(())
    });
    if let Err(message) = served {
        let _ = child.kill();
        panic!("{message}");
    }

    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("the engine didn't stop after StopEngine");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(status.success(), "engine exited with {status}");
    // It let go of the port: the next engine can take it.
    std::net::TcpListener::bind(("127.0.0.1", port)).expect("port released");
}

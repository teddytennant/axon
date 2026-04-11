use tauri::Manager;

/// Return the Axon node URL from env or default
#[tauri::command]
fn get_axon_url() -> String {
    std::env::var("AXON_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

/// Return the app version
#[tauri::command]
fn get_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// TCP-probe common Axon ports (non-blocking, 80ms timeout each).
/// Returns the first base URL that has something listening, or empty string.
#[tauri::command]
async fn probe_axon_ports() -> String {
    for port in [3000u16, 3001, 4000, 8080, 8000, 9000] {
        let addr = format!("127.0.0.1:{port}");
        if let Ok(sock) = addr.parse::<std::net::SocketAddr>() {
            if std::net::TcpStream::connect_timeout(&sock, std::time::Duration::from_millis(80))
                .is_ok()
            {
                return format!("http://localhost:{port}");
            }
        }
    }
    String::new()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .invoke_handler(tauri::generate_handler![get_axon_url, get_version, probe_axon_ports])
        .setup(|app| {
            let win = app.get_webview_window("main").unwrap();
            win.show().ok();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running axon desktop");
}

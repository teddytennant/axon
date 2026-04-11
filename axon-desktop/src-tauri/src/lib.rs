use tauri::Manager;

#[tauri::command]
fn get_axon_url() -> String {
    std::env::var("AXON_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![get_axon_url])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            window.set_title("axon").ok();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running axon desktop");
}

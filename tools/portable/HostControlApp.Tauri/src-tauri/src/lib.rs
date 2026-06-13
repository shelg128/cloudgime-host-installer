mod host_control;

use tauri::Manager;

use host_control::{
    change_admin_password, claim_setup_token, get_shell_state, launch_emergency_uninstaller,
    lock_app, preflight_host, recover_host_activation, redeem_activation_token,
    reset_local_host_identity, run_host_action, save_audio_preferences, save_display_preferences,
    save_preferences, send_heartbeat, set_admin_password, sync_host_binding,
    toggle_dual_stream, uninstall_installed_host, unlock_app, upload_host_diagnostic, AppSession,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppSession::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_skip_taskbar(false);
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            get_shell_state,
            set_admin_password,
            unlock_app,
            lock_app,
            change_admin_password,
            run_host_action,
            preflight_host,
            save_preferences,
            save_audio_preferences,
            save_display_preferences,
            toggle_dual_stream,
            sync_host_binding,
            claim_setup_token,
            recover_host_activation,
            reset_local_host_identity,
            redeem_activation_token,
            send_heartbeat,
            upload_host_diagnostic,
            uninstall_installed_host,
            launch_emergency_uninstaller
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

use std::sync::Arc;

use aegis_core::models::{
    AccountSearchResult, AppSettings, ChatMessage, DhtNodeContact, IdentityCreatedBundle,
    NetworkTelemetry, PeerSummary, PresenceStatus, ServerHealthResult, SignedProfileCard,
    VaultUnlockMode, VolunteerRelayResult, VpnAdapterInfo,
};
use aegis_core::storage::DuressAction;
use aegis_core::AegisNode;
use serde::Serialize;
use tauri::{Emitter, Manager, State};

pub struct AppState {
    pub node: Arc<AegisNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VaultStatusInfo {
    pub initialized: bool,
    pub unlocked: bool,
    pub unlock_mode: Option<VaultUnlockMode>,
    pub platform: String,
}

#[tauri::command]
async fn get_vault_status(state: State<'_, AppState>) -> Result<VaultStatusInfo, String> {
    Ok(VaultStatusInfo {
        initialized: state.node.is_initialized(),
        unlocked: state.node.is_unlocked().await,
        unlock_mode: state.node.unlock_mode().await,
        platform: std::env::consts::OS.to_string(),
    })
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn create_identity_and_vault(
    state: State<'_, AppState>,
    pin: String,
    display_name: String,
    handle: String,
    bio: String,
    role_badge: String,
    avatar_color: String,
    duress_pin: Option<String>,
    duress_action: Option<DuressAction>,
) -> Result<IdentityCreatedBundle, String> {
    let duress = match (duress_pin, duress_action) {
        (Some(dp), Some(act)) if !dp.trim().is_empty() => Some((dp, act)),
        _ => None,
    };
    state
        .node
        .create_identity_and_vault(&pin, display_name, handle, bio, role_badge, avatar_color, duress)
        .await
        .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn recover_identity_and_vault(
    state: State<'_, AppState>,
    mnemonic: String,
    pin: String,
    display_name: String,
    handle: String,
    bio: String,
    role_badge: String,
    avatar_color: String,
) -> Result<IdentityCreatedBundle, String> {
    state
        .node
        .recover_identity_and_vault(&mnemonic, &pin, display_name, handle, bio, role_badge, avatar_color)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn unlock_vault(state: State<'_, AppState>, pin: String) -> Result<VaultUnlockMode, String> {
    state
        .node
        .unlock_vault(&pin)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn lock_vault(state: State<'_, AppState>) -> Result<(), String> {
    state.node.lock_vault().await;
    Ok(())
}

#[tauri::command]
async fn reset_vault(state: State<'_, AppState>) -> Result<(), String> {
    state.node.reset_vault().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn configure_duress_pin(
    state: State<'_, AppState>,
    duress_pin: String,
    action: DuressAction,
) -> Result<(), String> {
    state
        .node
        .configure_duress_pin(&duress_pin, action)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_own_profile(state: State<'_, AppState>) -> Result<SignedProfileCard, String> {
    state.node.get_own_profile().await.map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn update_own_profile(
    state: State<'_, AppState>,
    display_name: String,
    handle: String,
    bio: String,
    role_badge: String,
    avatar_color: String,
    avatar_data: Option<String>,
    status: PresenceStatus,
) -> Result<SignedProfileCard, String> {
    state
        .node
        .update_own_profile(
            display_name,
            handle,
            bio,
            role_badge,
            avatar_color,
            avatar_data,
            status,
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    state.node.get_settings().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    state
        .node
        .update_settings(settings)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_peers(state: State<'_, AppState>) -> Result<Vec<PeerSummary>, String> {
    state.node.list_peers().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_messages(
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<Vec<ChatMessage>, String> {
    state
        .node
        .list_messages(&chat_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_peer_by_uri(state: State<'_, AppState>, uri: String) -> Result<PeerSummary, String> {
    state
        .node
        .add_peer_by_uri(&uri)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect_peer_addr(state: State<'_, AppState>, addr: String) -> Result<PeerSummary, String> {
    state
        .node
        .connect_peer_addr(&addr)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect_peer_by_id(
    state: State<'_, AppState>,
    peer_id: String,
) -> Result<PeerSummary, String> {
    state
        .node
        .connect_peer_by_id(&peer_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_accounts(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<AccountSearchResult>, String> {
    state
        .node
        .search_accounts(&query)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn connect_by_account(
    state: State<'_, AppState>,
    identifier: String,
) -> Result<PeerSummary, String> {
    state
        .node
        .connect_by_account(&identifier)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn initiate_udp_punch(
    state: State<'_, AppState>,
    peer_id: String,
) -> Result<PeerSummary, String> {
    state
        .node
        .initiate_udp_punch(&peer_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_bootstrap_peer(
    state: State<'_, AppState>,
    target: String,
) -> Result<Vec<String>, String> {
    state
        .node
        .add_bootstrap_peer(&target)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_bootstrap_peer(
    state: State<'_, AppState>,
    target: String,
) -> Result<Vec<String>, String> {
    state
        .node
        .remove_bootstrap_peer(&target)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn register_volunteer_relay(
    state: State<'_, AppState>,
    target: String,
) -> Result<VolunteerRelayResult, String> {
    state
        .node
        .register_volunteer_relay(&target)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn test_server_health(
    state: State<'_, AppState>,
    target: String,
) -> Result<ServerHealthResult, String> {
    state
        .node
        .check_server_health(&target)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn check_vpn_adapters(state: State<'_, AppState>) -> VpnAdapterInfo {
    state.node.check_local_vpn_adapters()
}

#[tauri::command]
async fn delete_chat(state: State<'_, AppState>, peer_id: String) -> Result<(), String> {
    state
        .node
        .delete_chat(&peer_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_dht_contacts(state: State<'_, AppState>) -> Result<Vec<DhtNodeContact>, String> {
    state
        .node
        .list_dht_contacts()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn export_seed_bundle(state: State<'_, AppState>) -> Result<String, String> {
    state
        .node
        .export_seed_bundle()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn import_seed_bundle(state: State<'_, AppState>, bundle: String) -> Result<usize, String> {
    state
        .node
        .import_seed_bundle(&bundle)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_message(
    state: State<'_, AppState>,
    peer_id: String,
    content: String,
    reply_to_id: Option<String>,
    expires_in_secs: Option<u64>,
) -> Result<ChatMessage, String> {
    state
        .node
        .send_message(&peer_id, content, reply_to_id, expires_in_secs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_typing(
    state: State<'_, AppState>,
    peer_id: String,
    is_typing: bool,
) -> Result<(), String> {
    state
        .node
        .send_typing(&peer_id, is_typing)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn toggle_reaction(
    state: State<'_, AppState>,
    chat_id: String,
    msg_id: String,
    emoji: String,
) -> Result<(), String> {
    state
        .node
        .toggle_reaction(&chat_id, &msg_id, emoji)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn edit_message(
    state: State<'_, AppState>,
    chat_id: String,
    msg_id: String,
    new_content: String,
) -> Result<(), String> {
    state
        .node
        .edit_message(&chat_id, &msg_id, new_content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_message(
    state: State<'_, AppState>,
    chat_id: String,
    msg_id: String,
    for_both: bool,
) -> Result<(), String> {
    state
        .node
        .delete_message(&chat_id, &msg_id, for_both)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn mark_chat_read(state: State<'_, AppState>, peer_id: String) -> Result<(), String> {
    state
        .node
        .mark_chat_read(&peer_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_peer_verified(
    state: State<'_, AppState>,
    peer_id: String,
    verified: bool,
) -> Result<PeerSummary, String> {
    state
        .node
        .set_peer_verified(&peer_id, verified)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_peer_pinned(
    state: State<'_, AppState>,
    peer_id: String,
    pinned: bool,
) -> Result<PeerSummary, String> {
    state
        .node
        .set_peer_pinned(&peer_id, pinned)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_network_telemetry(state: State<'_, AppState>) -> Result<NetworkTelemetry, String> {
    state
        .node
        .get_network_telemetry()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn trigger_discovery_pulse(state: State<'_, AppState>) -> Result<(), String> {
    state
        .node
        .trigger_discovery_pulse()
        .await
        .map_err(|e| e.to_string())
}

/// One-click user-level Windows installer helper when running the portable EXE:
/// copies executable to %LOCALAPPDATA%\Cymesh and creates Desktop + Start Menu shortcuts.
#[tauri::command]
async fn install_windows_shortcuts() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
        let local_app_data = std::env::var("LOCALAPPDATA").map_err(|e| e.to_string())?;
        let target_dir = std::path::PathBuf::from(local_app_data).join("CymeshOpenSource");
        std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
        let target_exe = target_dir.join("Cymesh.exe");
        if exe_path != target_exe {
            std::fs::copy(&exe_path, &target_exe).map_err(|e| e.to_string())?;
        }
        if let Some(exe_dir) = exe_path.parent() {
            let loader_src = exe_dir.join("WebView2Loader.dll");
            let loader_dst = target_dir.join("WebView2Loader.dll");
            if loader_src.exists() && loader_src != loader_dst {
                std::fs::copy(&loader_src, &loader_dst).map_err(|e| e.to_string())?;
            }
        }
        let target_str = target_exe.to_string_lossy().replace('\'', "''");
        let dir_str = target_dir.to_string_lossy().replace('\'', "''");
        let ps_script = format!(
            "$ws = New-Object -ComObject WScript.Shell; \
             $desk = [Environment]::GetFolderPath('Desktop'); \
             $s1 = $ws.CreateShortcut(\"$desk\\Cymesh Open Source.lnk\"); \
             $s1.TargetPath = '{target_str}'; \
             $s1.WorkingDirectory = '{dir_str}'; \
             $s1.Description = 'Cymesh Zero-Server Encrypted Messenger'; \
             $s1.Save(); \
             $prog = [Environment]::GetFolderPath('Programs'); \
             $s2 = $ws.CreateShortcut(\"$prog\\Cymesh Open Source.lnk\"); \
             $s2.TargetPath = '{target_str}'; \
             $s2.WorkingDirectory = '{dir_str}'; \
             $s2.Description = 'Cymesh Zero-Server Encrypted Messenger'; \
             $s2.Save();"
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_script])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("PowerShell shortcut creation failed".to_string());
        }
        Ok(target_exe.to_string_lossy().to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Windows shortcut installation is only available on Windows.".to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app
                .path()
                .app_local_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("./cymesh-vault"));
            let node = Arc::new(AegisNode::new_default(data_dir));

            let mut rx = node.subscribe();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(evt) = rx.recv().await {
                    let _ = app_handle.emit("aegis-event", &evt);
                }
            });

            app.manage(AppState { node });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_vault_status,
            create_identity_and_vault,
            recover_identity_and_vault,
            unlock_vault,
            lock_vault,
            reset_vault,
            configure_duress_pin,
            get_own_profile,
            update_own_profile,
            get_settings,
            update_settings,
            list_peers,
            list_messages,
            delete_chat,
            add_peer_by_uri,
            connect_peer_addr,
            connect_peer_by_id,
            search_accounts,
            connect_by_account,
            initiate_udp_punch,
            add_bootstrap_peer,
            remove_bootstrap_peer,
            register_volunteer_relay,
            test_server_health,
            check_vpn_adapters,
            list_dht_contacts,
            export_seed_bundle,
            import_seed_bundle,
            send_message,
            send_typing,
            toggle_reaction,
            edit_message,
            delete_message,
            mark_chat_read,
            set_peer_verified,
            set_peer_pinned,
            get_network_telemetry,
            trigger_discovery_pulse,
            install_windows_shortcuts,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let state = window.state::<AppState>();
                let node = state.node.clone();
                tauri::async_runtime::block_on(async move {
                    node.withdraw_presence().await;
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Cymesh application");
}

pub mod crypto;
pub mod dht;
pub mod error;
pub mod identity;
pub mod models;
pub mod storage;
pub mod transport;
pub mod upnp;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;
use ed25519_dalek::Signer;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Static};

use crate::crypto::{
    compute_route_tag, seal_for_recipient, unseal_packet, DoubleRatchetState, SealedInnerPayload,
    SealedMeshPacket,
};
use crate::dht::{
    build_onion_packet, peel_onion_layer, sign_punch_handshake, verify_punch_handshake,
    DhtRecordStore, DhtRoutingTable, OnionLayerPayload, OnionPacket, SignedDhtRecord,
    UdpMeshPacket, UdpTunnelCipher,
};
use crate::error::{AegisError, Result};
use crate::identity::{
    compute_safety_verification, decode_32_b58, decode_share_uri, encode_share_uri,
    format_account_id, format_fingerprint, IdentityKeys,
};
use crate::models::{
    AccountSearchResult, AppSettings, ChatMessage, ConnectionRoute, CoreEvent, DeliveryStatus,
    DhtNodeContact, IdentityCreatedBundle, MessageReaction, NetworkTelemetry, PeerSummary,
    PresenceStatus, RelayHint, ServerHealthResult, SignedProfileCard, VaultUnlockMode,
    VolunteerRelayResult, VpnAdapterInfo,
};
use crate::storage::{DuressAction, OutboxItem, UnlockedVault};
use crate::transport::{
    bind_discovery_socket, bind_mesh_udp_socket, bind_tcp_listener, enumerate_broadcast_addrs,
    enumerate_local_ipv4s, is_private_or_local_ip, DiscoveryBeacon, NoiseChannel, WireFrame,
    DEFAULT_DISCOVERY_PORT,
};

const DHT_RECORD_TTL_MS: i64 = 5 * 60 * 1000;

#[derive(Clone)]
struct ActivePeerConn {
    tx: mpsc::Sender<WireFrame>,
    route: ConnectionRoute,
    remote_addr: Option<String>,
    udp_port: u16,
    accepts_relay: bool,
    x25519_pub: [u8; 32],
    is_udp: bool,
}

struct PendingPunchState {
    target_peer_id: String,
    target_static_x: [u8; 32],
    my_eph_secret: X25519Static,
    my_eph_pub_b58: String,
    target_udp_addr: Option<SocketAddr>,
    created_at: i64,
}

#[derive(Clone)]
struct ActiveUdpTunnel {
    cipher: UdpTunnelCipher,
    remote_udp_addr: SocketAddr,
    last_rx_ms: i64,
}

#[derive(Clone)]
struct NodeContext {
    keys: Arc<IdentityKeys>,
    vault: Arc<Mutex<UnlockedVault>>,
    own_profile: Arc<RwLock<SignedProfileCard>>,
    settings: Arc<RwLock<AppSettings>>,
    tcp_port: u16,
    udp_port: u16,
    udp_socket: Arc<UdpSocket>,
    discovery_port: u16,
    active_peers: Arc<tokio::sync::RwLock<HashMap<String, ActivePeerConn>>>,
    peer_latencies: Arc<RwLock<HashMap<String, u32>>>,
    seen_mesh_ids: Arc<Mutex<HashSet<String>>>,
    mesh_relayed_count: Arc<AtomicU64>,
    nat_punch_successes: Arc<AtomicU64>,
    onion_routed_count: Arc<AtomicU64>,
    peer_handles: Arc<Mutex<Vec<tokio::task::AbortHandle>>>,
    routing_table: Arc<RwLock<DhtRoutingTable>>,
    dht_records: Arc<RwLock<DhtRecordStore>>,
    observed_external_addr: Arc<RwLock<Option<String>>>,
    pending_dht_queries: Arc<Mutex<HashMap<String, (String, oneshot::Sender<Option<SignedDhtRecord>>)>>>,
    pending_account_searches: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Vec<SignedDhtRecord>>>>>,
    search_reply_routes: Arc<Mutex<HashMap<String, String>>>,
    seen_search_ids: Arc<Mutex<HashSet<String>>>,
    pending_punches: Arc<Mutex<HashMap<String, PendingPunchState>>>,
    punch_relay_routes: Arc<Mutex<HashMap<String, String>>>,
    udp_tunnels: Arc<RwLock<HashMap<String, ActiveUdpTunnel>>>,
    pending_echo_nonces: Arc<Mutex<HashMap<u64, i64>>>,
    dht_seq: Arc<AtomicU64>,
    events_tx: broadcast::Sender<CoreEvent>,
}

struct ActiveRuntime {
    ctx: Arc<NodeContext>,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for ActiveRuntime {
    fn drop(&mut self) {
        for handle in self.tasks.drain(..) {
            handle.abort();
        }
        if let Ok(mut list) = self.ctx.peer_handles.lock() {
            for handle in list.drain(..) {
                handle.abort();
            }
        }
    }
}

pub struct AegisNode {
    data_dir: PathBuf,
    discovery_port: u16,
    initial_bootstrap_peer: Option<SocketAddr>,
    runtime: Arc<tokio::sync::RwLock<Option<ActiveRuntime>>>,
    events_tx: broadcast::Sender<CoreEvent>,
}

impl AegisNode {
    pub fn new(data_dir: PathBuf, discovery_port: u16) -> Self {
        let (events_tx, _) = broadcast::channel(512);
        Self {
            data_dir,
            discovery_port,
            initial_bootstrap_peer: None,
            runtime: Arc::new(tokio::sync::RwLock::new(None)),
            events_tx,
        }
    }

    pub fn new_default(data_dir: PathBuf) -> Self {
        Self::new(data_dir, DEFAULT_DISCOVERY_PORT)
    }

    /// Sets a bridge for newly created or recovered primary vaults.
    pub fn with_initial_bootstrap_peer(mut self, peer: SocketAddr) -> Self {
        self.initial_bootstrap_peer = Some(peer);
        self
    }

    fn create_vault(
        &self,
        pin: &str,
        keys: &IdentityKeys,
        profile: &SignedProfileCard,
        duress_pin: Option<(&str, DuressAction)>,
    ) -> Result<()> {
        let mut settings = AppSettings::default();
        settings
            .bootstrap_peers
            .extend(self.initial_bootstrap_peer.map(|peer| peer.to_string()));
        UnlockedVault::create_new_with_settings(
            &self.data_dir, pin, keys, profile, duress_pin, &settings,
        )?;
        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events_tx.subscribe()
    }

    pub fn is_initialized(&self) -> bool {
        UnlockedVault::is_initialized(&self.data_dir)
    }

    pub async fn is_unlocked(&self) -> bool {
        self.runtime.read().await.is_some()
    }

    pub async fn unlock_mode(&self) -> Option<VaultUnlockMode> {
        self.runtime.read().await.as_ref().map(|rt| {
            rt.ctx
                .vault
                .lock()
                .map(|v| v.mode)
                .unwrap_or(VaultUnlockMode::Primary)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_identity_and_vault(
        &self,
        pin: &str,
        display_name: String,
        handle: String,
        bio: String,
        role_badge: String,
        avatar_color: String,
        duress_pin: Option<(String, DuressAction)>,
    ) -> Result<IdentityCreatedBundle> {
        let (keys, mnemonic) = IdentityKeys::generate()?;
        let profile = keys.build_signed_profile(
            display_name,
            normalize_handle(&handle),
            bio,
            role_badge,
            avatar_color,
            None,
            PresenceStatus::Online,
        );

        let duress_ref = duress_pin.as_ref().map(|(p, a)| (p.as_str(), *a));
        self.create_vault(pin, &keys, &profile, duress_ref)?;

        self.unlock_vault(pin).await?;

        let telemetry = self.get_network_telemetry().await?;
        let account_id = format_account_id(&profile.handle, &profile.peer_id);
        Ok(IdentityCreatedBundle {
            fingerprint: keys.fingerprint(),
            share_uri: telemetry.share_uri,
            account_id,
            profile,
            recovery_mnemonic: mnemonic,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn recover_identity_and_vault(
        &self,
        mnemonic: &str,
        pin: &str,
        display_name: String,
        handle: String,
        bio: String,
        role_badge: String,
        avatar_color: String,
    ) -> Result<IdentityCreatedBundle> {
        let keys = IdentityKeys::from_mnemonic(mnemonic)?;
        let profile = keys.build_signed_profile(
            display_name,
            normalize_handle(&handle),
            bio,
            role_badge,
            avatar_color,
            None,
            PresenceStatus::Online,
        );

        self.create_vault(pin, &keys, &profile, None)?;

        self.unlock_vault(pin).await?;

        let telemetry = self.get_network_telemetry().await?;
        let account_id = format_account_id(&profile.handle, &profile.peer_id);
        Ok(IdentityCreatedBundle {
            fingerprint: keys.fingerprint(),
            share_uri: telemetry.share_uri,
            account_id,
            profile,
            recovery_mnemonic: mnemonic.trim().to_string(),
        })
    }

    pub async fn unlock_vault(&self, pin: &str) -> Result<VaultUnlockMode> {
        self.lock_vault().await;

        let mut attempts = 0;
        let (vault, keys) = loop {
            match UnlockedVault::unlock(&self.data_dir, pin) {
                Ok(res) => break res,
                Err(e) => {
                    attempts += 1;
                    if attempts > 6 {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_millis(60)).await;
                }
            }
        };
        let mode = vault.mode;
        let own_profile = vault.load_own_profile()?;
        let settings = vault.load_settings()?;
        let now = Utc::now().timestamp_millis();

        // Hydrate persistent DHT routing table & cached signed peer records for blackout resilience
        let my_id = keys.peer_id();
        let mut rt_table = DhtRoutingTable::new(my_id);
        for contact in vault.load_dht_contacts().unwrap_or_default() {
            rt_table.upsert(contact);
        }
        let mut rec_store = DhtRecordStore::default();
        for rec in vault.load_dht_records(now).unwrap_or_default() {
            let _ = rec_store.upsert(rec, now);
        }

        let (listener, tcp_port) = bind_tcp_listener(settings.listen_port).await?;
        let (udp_socket, udp_port) = bind_mesh_udp_socket(tcp_port)?;

        let ctx = Arc::new(NodeContext {
            keys: Arc::new(keys),
            vault: Arc::new(Mutex::new(vault)),
            own_profile: Arc::new(RwLock::new(own_profile)),
            settings: Arc::new(RwLock::new(settings.clone())),
            tcp_port,
            udp_port,
            udp_socket: Arc::new(udp_socket),
            discovery_port: self.discovery_port,
            active_peers: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            peer_latencies: Arc::new(RwLock::new(HashMap::new())),
            seen_mesh_ids: Arc::new(Mutex::new(HashSet::new())),
            mesh_relayed_count: Arc::new(AtomicU64::new(0)),
            nat_punch_successes: Arc::new(AtomicU64::new(0)),
            onion_routed_count: Arc::new(AtomicU64::new(0)),
            peer_handles: Arc::new(Mutex::new(Vec::new())),
            routing_table: Arc::new(RwLock::new(rt_table)),
            dht_records: Arc::new(RwLock::new(rec_store)),
            observed_external_addr: Arc::new(RwLock::new(None)),
            pending_dht_queries: Arc::new(Mutex::new(HashMap::new())),
            pending_account_searches: Arc::new(Mutex::new(HashMap::new())),
            search_reply_routes: Arc::new(Mutex::new(HashMap::new())),
            seen_search_ids: Arc::new(Mutex::new(HashSet::new())),
            pending_punches: Arc::new(Mutex::new(HashMap::new())),
            punch_relay_routes: Arc::new(Mutex::new(HashMap::new())),
            udp_tunnels: Arc::new(RwLock::new(HashMap::new())),
            pending_echo_nonces: Arc::new(Mutex::new(HashMap::new())),
            dht_seq: Arc::new(AtomicU64::new(1)),
            events_tx: self.events_tx.clone(),
        });

        // Index own signed account record locally and dial cached mesh contacts
        announce_own_dht_record(&ctx).await;
        trigger_mesh_bootstrap(&ctx, &settings.bootstrap_peers).await;

        let tasks = vec![
            spawn_tcp_accept_loop(ctx.clone(), listener),
            spawn_udp_mesh_loop(ctx.clone()),
            spawn_discovery_loop(ctx.clone()),
            spawn_maintenance_loop(ctx.clone()),
            spawn_upnp_loop(ctx.clone()),
        ];

        *self.runtime.write().await = Some(ActiveRuntime { ctx, tasks });
        Ok(mode)
    }

    /// Explicitly withdraws own presence from connected relays and DHT peers so
    /// this account does not linger as an offline ghost in other users' searches.
    pub async fn withdraw_presence(&self) {
        if let Some(ref rt) = *self.runtime.read().await {
            broadcast_dht_withdrawal(&rt.ctx).await;
        }
    }

    pub async fn lock_vault(&self) {
        let old = self.runtime.write().await.take();
        if let Some(mut rt) = old {
            if let Ok(mut list) = rt.ctx.peer_handles.lock() {
                for handle in list.drain(..) {
                    handle.abort();
                }
            }
            let tasks: Vec<_> = rt.tasks.drain(..).collect();
            for handle in &tasks {
                handle.abort();
            }
            let _ = tokio::time::timeout(Duration::from_millis(300), async {
                for handle in tasks {
                    let _ = handle.await;
                }
            })
            .await;
            drop(rt);
        }
        let _ = self.events_tx.send(CoreEvent::VaultLocked);
    }

    /// Locks any active session and securely deletes local vault files so the user
    /// can restore from their 12-word key or create a new identity after a forgotten PIN.
    pub async fn reset_vault(&self) -> Result<()> {
        self.withdraw_presence().await;
        self.lock_vault().await;
        UnlockedVault::wipe_all(&self.data_dir);
        Ok(())
    }

    pub async fn configure_duress_pin(&self, duress_pin: &str, action: DuressAction) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let v = rt
            .ctx
            .vault
            .lock()
            .map_err(|_| AegisError::Storage("Vault lock".into()))?;
        v.configure_duress_pin(duress_pin, action)
    }

    pub async fn get_own_profile(&self) -> Result<SignedProfileCard> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let p = rt
            .ctx
            .own_profile
            .read()
            .map_err(|_| AegisError::Storage("Profile lock".into()))?
            .clone();
        Ok(p)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_own_profile(
        &self,
        display_name: String,
        handle: String,
        bio: String,
        role_badge: String,
        avatar_color: String,
        avatar_data: Option<String>,
        status: PresenceStatus,
    ) -> Result<SignedProfileCard> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;

        let clean_bio = bio.trim().to_string();
        if clean_bio.chars().count() > 500 {
            return Err(AegisError::InvalidPayload("Bio must be at most 500 characters".into()));
        }

        let clean_avatar = match avatar_data {
            Some(ref data) if !data.trim().is_empty() => {
                let trimmed = data.trim();
                if trimmed.len() > 65536 {
                    return Err(AegisError::InvalidPayload("Avatar data must be at most 64KB".into()));
                }
                if !trimmed.starts_with("data:image/webp;base64,")
                    && !trimmed.starts_with("data:image/jpeg;base64,")
                    && !trimmed.starts_with("data:image/jpg;base64,")
                    && !trimmed.starts_with("data:image/png;base64,")
                {
                    return Err(AegisError::InvalidPayload(
                        "Avatar data must be a safe base64 raster image data URI".into(),
                    ));
                }
                Some(trimmed.to_string())
            }
            _ => None,
        };

        let updated = rt.ctx.keys.build_signed_profile(
            display_name,
            normalize_handle(&handle),
            clean_bio,
            role_badge,
            avatar_color,
            clean_avatar,
            status,
        );
        {
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            v.save_own_profile(&updated)?;
        }
        if let Ok(mut w) = rt.ctx.own_profile.write() {
            *w = updated.clone();
        }

        let senders: Vec<mpsc::Sender<WireFrame>> = rt
            .ctx
            .active_peers
            .read()
            .await
            .values()
            .map(|p| p.tx.clone())
            .collect();
        for tx in senders {
            let _ = tx.send(WireFrame::ProfileSync(updated.clone())).await;
        }
        announce_own_dht_record(&rt.ctx).await;
        Ok(updated)
    }

    pub async fn get_settings(&self) -> Result<AppSettings> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let s = rt
            .ctx
            .settings
            .read()
            .map_err(|_| AegisError::Storage("Settings lock".into()))?
            .clone();
        Ok(s)
    }

    pub async fn update_settings(&self, new_settings: AppSettings) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        {
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            v.save_settings(&new_settings)?;
        }
        let seeds = new_settings.bootstrap_peers.clone();
        if let Ok(mut w) = rt.ctx.settings.write() {
            *w = new_settings;
        }
        trigger_mesh_bootstrap(&rt.ctx, &seeds).await;
        announce_own_dht_record(&rt.ctx).await;
        Ok(())
    }

    /// Adds a mesh entry node (`IP:Port`, `cymesh://...`, or `CYMESH-SEED:...`) without any central server.
    pub async fn add_bootstrap_peer(&self, target: &str) -> Result<Vec<String>> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Err(AegisError::Network("Entry target cannot be empty".into()));
        }

        if let Some(encoded) = trimmed
            .strip_prefix("CYMESH-SEED:")
            .or_else(|| trimmed.strip_prefix("AEGIS-SEED:"))
        {
            let raw = bs58::decode(encoded.trim())
                .into_vec()
                .map_err(|e| AegisError::InvalidKey(e.to_string()))?;
            let decoded = String::from_utf8(raw)
                .map_err(|e| AegisError::InvalidKey(e.to_string()))?;
            let mut last = Vec::new();
            for line in decoded.lines().flat_map(|l| l.split(',')) {
                let entry = line.trim();
                if !entry.is_empty() {
                    if let Ok(updated) = Box::pin(self.add_bootstrap_peer(entry)).await {
                        last = updated;
                    }
                }
            }
            return Ok(last);
        }

        let mut addrs_to_save = Vec::new();
        if trimmed.starts_with("cymesh://") || trimmed.starts_with("aegis://") {
            let payload = decode_share_uri(trimmed)?;
            if payload.addrs.is_empty() {
                return Err(AegisError::Network("Share URI contains no addresses".into()));
            }
            for a in payload.addrs {
                if a.parse::<SocketAddr>().is_ok() {
                    addrs_to_save.push(a);
                }
            }
        } else {
            let sock: SocketAddr = trimmed
                .parse()
                .map_err(|_| AegisError::Network(format!("Invalid entry address '{trimmed}'")))?;
            addrs_to_save.push(sock.to_string());
        }

        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let updated_seeds = {
            let mut s = rt
                .ctx
                .settings
                .write()
                .map_err(|_| AegisError::Storage("Settings lock".into()))?;
            for a in &addrs_to_save {
                if !s.bootstrap_peers.contains(a) {
                    s.bootstrap_peers.push(a.clone());
                }
            }
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            v.save_settings(&s)?;
            s.bootstrap_peers.clone()
        };

        for a in addrs_to_save {
            if let Ok(sock) = a.parse::<SocketAddr>() {
                connect_and_register_peer(rt.ctx.clone(), sock);
            }
        }
        Ok(updated_seeds)
    }

    /// Verifies connectivity to a volunteer blind relay (`IP:Port`), registers it as a persistent
    /// bootstrap bridge, and propagates its DHT contact/record across connected mesh peers.
    pub async fn register_volunteer_relay(&self, target: &str) -> Result<VolunteerRelayResult> {
        let trimmed = target.trim();
        let sock: SocketAddr = trimmed
            .parse()
            .map_err(|_| AegisError::Network(format!("Invalid relay address '{trimmed}' (expected IP:Port)")))?;

        let (ctx, my_prof) = {
            let guard = self.runtime.read().await;
            let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
            let my_prof = rt
                .ctx
                .own_profile
                .read()
                .map_err(|_| AegisError::Storage("Profile lock".into()))?
                .clone();
            (rt.ctx.clone(), my_prof)
        };

        let started = tokio::time::Instant::now();
        let channel = tokio::time::timeout(
            Duration::from_secs(6),
            NoiseChannel::connect(sock, &ctx.keys.x25519_secret(), &ctx.keys.signing_key(), &my_prof),
        )
        .await
        .map_err(|_| AegisError::Network(format!("Connection timed out dialing {sock}")))??;
        let latency_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;

        let summary = register_connected_channel(&ctx, channel, Some(sock.to_string())).await?;
        let relay_peer_id = summary.profile.peer_id.clone();

        {
            if let Ok(mut lat) = ctx.peer_latencies.write() {
                lat.insert(relay_peer_id.clone(), latency_ms);
            }
            if let Ok(mut table) = ctx.routing_table.write() {
                table.upsert(DhtNodeContact {
                    peer_id: relay_peer_id.clone(),
                    x25519_pub: summary.profile.x25519_pub.clone(),
                    tcp_addrs: vec![sock.to_string()],
                    udp_addr: None,
                    relay_capable: true,
                    last_seen: Utc::now().timestamp_millis(),
                    failures: 0,
                });
            }
            let mut s = ctx
                .settings
                .write()
                .map_err(|_| AegisError::Storage("Settings lock".into()))?;
            let addr_str = sock.to_string();
            if !s.bootstrap_peers.contains(&addr_str) {
                s.bootstrap_peers.push(addr_str);
            }
            let v = ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            v.save_settings(&s)?;
            if let Ok(table) = ctx.routing_table.read() {
                let _ = v.save_dht_contacts(&table.all_contacts());
            }
        }

        announce_own_dht_record(&ctx).await;

        let contacts_to_share = ctx
            .routing_table
            .read()
            .map(|t| t.find_closest(&relay_peer_id, 16))
            .unwrap_or_default();
        let active_senders: Vec<(String, mpsc::Sender<WireFrame>)> = ctx
            .active_peers
            .read()
            .await
            .iter()
            .map(|(id, c)| (id.clone(), c.tx.clone()))
            .collect();
        let mut propagated_peers = 0usize;
        for (pid, tx) in active_senders {
            if pid != relay_peer_id {
                let _ = tx
                    .send(WireFrame::DhtFindNodeResp {
                        req_id: Uuid::new_v4().to_string(),
                        contacts: contacts_to_share.clone(),
                    })
                    .await;
                propagated_peers += 1;
            }
        }

        let telemetry = compute_telemetry(&ctx).await?;
        let _ = ctx.events_tx.send(CoreEvent::NetworkStatus(telemetry));

        Ok(VolunteerRelayResult {
            addr: sock.to_string(),
            peer_id: relay_peer_id,
            latency_ms,
            propagated_peers,
        })
    }

    /// Checks whether active VPN or TUN/TAP network adapters are running on the local host.
    pub fn check_local_vpn_adapters(&self) -> VpnAdapterInfo {
        crate::transport::check_local_vpn_adapters()
    }

    /// Checks the health and reachability of a mesh relay or bootstrap node (TCP ping and Noise handshake).
    pub async fn check_server_health(&self, target: &str) -> Result<ServerHealthResult> {
        let trimmed = target.trim();
        let sock: SocketAddr = match trimmed.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ServerHealthResult {
                    target: trimmed.to_string(),
                    reachable: false,
                    latency_ms: None,
                    noise_handshake_ok: false,
                    peer_id: None,
                    handle: None,
                    display_name: None,
                    role: None,
                    observed_ip: None,
                    message: format!("آدرس سرور نامعتبر است: {e}"),
                    timestamp: Utc::now().timestamp_millis(),
                });
            }
        };

        let (ctx, my_prof) = {
            let guard = self.runtime.read().await;
            let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
            let my_prof = rt
                .ctx
                .own_profile
                .read()
                .map_err(|_| AegisError::Storage("Profile lock".into()))?
                .clone();
            (rt.ctx.clone(), my_prof)
        };

        let started = tokio::time::Instant::now();
        let channel_res = tokio::time::timeout(
            Duration::from_secs(6),
            NoiseChannel::connect(sock, &ctx.keys.x25519_secret(), &ctx.keys.signing_key(), &my_prof),
        )
        .await;

        let channel = match channel_res {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                return Ok(ServerHealthResult {
                    target: trimmed.to_string(),
                    reachable: false,
                    latency_ms: None,
                    noise_handshake_ok: false,
                    peer_id: None,
                    handle: None,
                    display_name: None,
                    role: None,
                    observed_ip: None,
                    message: format!("خطا در اتصال: {e}"),
                    timestamp: Utc::now().timestamp_millis(),
                });
            }
            Err(_) => {
                return Ok(ServerHealthResult {
                    target: trimmed.to_string(),
                    reachable: false,
                    latency_ms: None,
                    noise_handshake_ok: false,
                    peer_id: None,
                    handle: None,
                    display_name: None,
                    role: None,
                    observed_ip: None,
                    message: "مهلت اتصال به پایان رسید (تایم‌اوت - سرور پاسخ نداد یا مسدود است)".to_string(),
                    timestamp: Utc::now().timestamp_millis(),
                });
            }
        };

        let latency_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
        let peer_profile = channel.peer_profile.clone();
        let peer_id = peer_profile.peer_id.clone();
        let handle = peer_profile.handle.clone();
        let display_name = peer_profile.display_name.clone();
        let role = peer_profile.role_badge.clone();

        // Register the verified connection so mesh relaying works immediately
        let _ = register_connected_channel(&ctx, channel, Some(sock.to_string())).await;

        let observed_ip = ctx.observed_external_addr.read().ok().and_then(|o| o.clone());

        Ok(ServerHealthResult {
            target: trimmed.to_string(),
            reachable: true,
            latency_ms: Some(latency_ms),
            noise_handshake_ok: true,
            peer_id: Some(peer_id),
            handle: Some(handle),
            display_name: Some(display_name),
            role: Some(role),
            observed_ip,
            message: "سرور فعال و آنلاین است. ارتباط امن نویز تایید شد.".to_string(),
            timestamp: Utc::now().timestamp_millis(),
        })
    }

    pub async fn remove_bootstrap_peer(&self, target: &str) -> Result<Vec<String>> {
        let trimmed = target.trim();
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let updated = {
            let mut s = rt
                .ctx
                .settings
                .write()
                .map_err(|_| AegisError::Storage("Settings lock".into()))?;
            s.bootstrap_peers.retain(|p| p != trimmed);
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            v.save_settings(&s)?;
            s.bootstrap_peers.clone()
        };
        Ok(updated)
    }

    pub async fn list_dht_contacts(&self) -> Result<Vec<DhtNodeContact>> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let list = rt
            .ctx
            .routing_table
            .read()
            .map_err(|_| AegisError::Storage("DHT lock".into()))?
            .all_contacts();
        Ok(list)
    }

    /// Exports active & known reachable IP:Port seed nodes so users can share working
    /// intranet entry points offline or over SMS during internet blackouts.
    pub async fn export_seed_bundle(&self) -> Result<String> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let mut lines = Vec::new();
        if let Some(ext) = rt
            .ctx
            .observed_external_addr
            .read()
            .ok()
            .and_then(|o| o.clone())
        {
            if let Ok(sock) = ext.parse::<SocketAddr>() {
                lines.push(SocketAddr::new(sock.ip(), rt.ctx.tcp_port).to_string());
            }
        }
        if let Ok(s) = rt.ctx.settings.read() {
            for b in &s.bootstrap_peers {
                if !lines.contains(b) {
                    lines.push(b.clone());
                }
            }
        }
        if let Ok(table) = rt.ctx.routing_table.read() {
            for c in table.all_contacts() {
                if c.relay_capable {
                    for a in c.tcp_addrs {
                        if !lines.contains(&a) && lines.len() < 32 {
                            lines.push(a);
                        }
                    }
                }
            }
        }
        if lines.is_empty() {
            return Ok(String::new());
        }
        let encoded = bs58::encode(lines.join("\n").as_bytes()).into_string();
        Ok(format!("CYMESH-SEED:{encoded}"))
    }

    pub async fn import_seed_bundle(&self, bundle: &str) -> Result<usize> {
        let trimmed_all = bundle.trim();
        let decoded_str = if let Some(encoded) = trimmed_all
            .strip_prefix("CYMESH-SEED:")
            .or_else(|| trimmed_all.strip_prefix("AEGIS-SEED:"))
        {
            let raw = bs58::decode(encoded.trim())
                .into_vec()
                .map_err(|e| AegisError::InvalidKey(e.to_string()))?;
            String::from_utf8(raw).map_err(|e| AegisError::InvalidKey(e.to_string()))?
        } else {
            trimmed_all.to_string()
        };

        let mut added = 0usize;
        for line in decoded_str.lines().flat_map(|l| l.split(',')) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if self.add_bootstrap_peer(trimmed).await.is_ok() {
                added += 1;
            }
        }
        Ok(added)
    }

    pub async fn list_peers(&self) -> Result<Vec<PeerSummary>> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let active_snapshot: HashMap<String, ConnectionRoute> = rt
            .ctx
            .active_peers
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), v.route))
            .collect();
        let has_relays = rt
            .ctx
            .active_peers
            .read()
            .await
            .values()
            .any(|v| v.accepts_relay);
        let latencies = rt
            .ctx
            .peer_latencies
            .read()
            .map(|l| l.clone())
            .unwrap_or_default();
        let now = Utc::now().timestamp_millis();

        let v = rt
            .ctx
            .vault
            .lock()
            .map_err(|_| AegisError::Storage("Vault lock".into()))?;
        let mut peers = v.load_all_peers()?;
        for p in &peers {
            if p.profile.is_hidden_relay() && p.last_message_at.is_none() {
                let _ = v.delete_chat(&p.profile.peer_id);
            }
        }
        peers.retain(|p| !p.profile.is_hidden_relay());
        for p in &mut peers {
            if p.account_id.is_empty() {
                p.account_id = format_account_id(&p.profile.handle, &p.profile.peer_id);
            }
            if let Some(route) = active_snapshot.get(&p.profile.peer_id) {
                p.route = *route;
                p.latency_ms = latencies.get(&p.profile.peer_id).copied();
            } else {
                let dht_reachable = has_relays
                    && rt
                        .ctx
                        .dht_records
                        .read()
                        .ok()
                        .and_then(|s| s.get(&p.profile.peer_id, now))
                        .map(|r| !r.relay_hints.is_empty() || r.accepts_relay || (now - r.published_at) < 3600_000)
                        .unwrap_or(false);
                p.route = if dht_reachable {
                    ConnectionRoute::MeshRelay
                } else {
                    ConnectionRoute::Offline
                };
                p.latency_ms = None;
            }
        }
        Ok(peers)
    }

    pub async fn list_messages(&self, chat_id: &str) -> Result<Vec<ChatMessage>> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let v = rt
            .ctx
            .vault
            .lock()
            .map_err(|_| AegisError::Storage("Vault lock".into()))?;
        v.load_messages(chat_id)
    }

    pub async fn add_peer_by_uri(&self, uri: &str) -> Result<PeerSummary> {
        let payload = decode_share_uri(uri)?;
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let my_id = rt.ctx.keys.peer_id();
        if payload.profile.peer_id == my_id {
            return Err(AegisError::InvalidKey("Cannot add yourself as a peer".into()));
        }

        let first_addr = payload.addrs.first().cloned();
        let summary = upsert_peer_in_vault(&rt.ctx.vault, &my_id, payload.profile.clone(), first_addr.clone())?;
        let _ = self.events_tx.send(CoreEvent::PeerUpdated(summary.clone()));

        if let Ok(mut table) = rt.ctx.routing_table.write() {
            table.upsert(DhtNodeContact {
                peer_id: payload.profile.peer_id.clone(),
                x25519_pub: payload.profile.x25519_pub.clone(),
                tcp_addrs: payload.addrs.clone(),
                udp_addr: None,
                relay_capable: true,
                last_seen: Utc::now().timestamp_millis(),
                failures: 0,
            });
        }

        for addr_str in &payload.addrs {
            if let Ok(sock) = addr_str.parse::<SocketAddr>() {
                connect_and_register_peer(rt.ctx.clone(), sock);
            }
        }

        let ctx_clone = rt.ctx.clone();
        let pid = payload.profile.peer_id;
        tokio::spawn(async move {
            let _ = lookup_peer_record_internal(&ctx_clone, &pid).await;
        });

        Ok(summary)
    }

    /// Queries the serverless Kademlia DHT for a peer record by Peer ID (cross-city lookup).
    pub async fn lookup_peer_in_dht(&self, peer_id: &str) -> Result<Option<SignedDhtRecord>> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        lookup_peer_record_internal(&rt.ctx, peer_id.trim()).await
    }

    /// Searches decentralized Cymesh accounts across the Kademlia DHT overlay by `@handle`,
    /// `@handle#TAG`, display name, or `Peer ID`. Works without any central directory server.
    pub async fn search_accounts(&self, query: &str) -> Result<Vec<AccountSearchResult>> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let my_id = rt.ctx.keys.peer_id();
        let trimmed = query.trim();
        let now = Utc::now().timestamp_millis();

        let paired_ids: HashSet<String> = rt
            .ctx
            .vault
            .lock()
            .ok()
            .and_then(|v| v.load_all_peers().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.profile.peer_id)
            .collect();

        let senders: Vec<mpsc::Sender<WireFrame>> = rt
            .ctx
            .active_peers
            .read()
            .await
            .values()
            .map(|c| c.tx.clone())
            .collect();

        if !senders.is_empty() {
            let req_id = Uuid::new_v4().to_string();
            let (resp_tx, mut resp_rx) = mpsc::unbounded_channel();
            if let Ok(mut pending) = rt.ctx.pending_account_searches.lock() {
                pending.insert(req_id.clone(), resp_tx);
            }
            if let Ok(mut seen) = rt.ctx.seen_search_ids.lock() {
                if seen.len() > 2048 {
                    seen.clear();
                }
                seen.insert(req_id.clone());
            }

            let q_is_exact = trimmed.contains('#') || decode_32_b58(trimmed).is_ok();
            let q_lower = trimmed.to_lowercase();
            for tx in senders {
                let _ = tx
                    .send(WireFrame::DhtSearchAccount {
                        req_id: req_id.clone(),
                        query: trimmed.to_string(),
                        ttl: 3,
                    })
                    .await;
            }

            let deadline = tokio::time::Instant::now() + Duration::from_millis(1800);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match tokio::time::timeout(remaining, resp_rx.recv()).await {
                    Ok(Some(recs)) => {
                        if q_is_exact
                            && recs.iter().any(|r| {
                                r.peer_id == trimmed
                                    || r.profile
                                        .as_ref()
                                        .map(|p| format_account_id(&p.handle, &r.peer_id).to_lowercase() == q_lower)
                                        .unwrap_or(false)
                            })
                        {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            if let Ok(mut pending) = rt.ctx.pending_account_searches.lock() {
                pending.remove(&req_id);
            }
        }

        let after_now = Utc::now().timestamp_millis().max(now);
        let mut results = Vec::new();
        let mut seen_peers = HashSet::new();

        if let Ok(store) = rt.ctx.dht_records.read() {
            for (rec, acct_id) in store.search_accounts(trimmed, after_now) {
                if rec.peer_id == my_id || !seen_peers.insert(rec.peer_id.clone()) {
                    continue;
                }
                let profile = rec.profile.clone().unwrap_or_else(|| SignedProfileCard {
                    peer_id: rec.peer_id.clone(),
                    x25519_pub: rec.x25519_pub.clone(),
                    display_name: format!("Node {}", &rec.peer_id[..6.min(rec.peer_id.len())]),
                    handle: "@node".to_string(),
                    bio: "Verified Cymesh Account".to_string(),
                    role_badge: "Cymesh".to_string(),
                    avatar_color: "#00F0FF".to_string(),
                    avatar_data: None,
                    status: PresenceStatus::Online,
                    updated_at: rec.published_at,
                    signature: rec.signature.clone(),
                });
                if profile.is_hidden_relay() {
                    continue;
                }
                results.push(AccountSearchResult {
                    peer_id: rec.peer_id.clone(),
                    account_id: acct_id,
                    profile,
                    direct_reachable: !rec.tcp_addrs.is_empty() || rec.udp_punch_addr.is_some(),
                    relay_hops_available: rec.relay_hints.len(),
                    already_paired: paired_ids.contains(&rec.peer_id),
                });
            }
        }

        // Also include already-paired contacts matching query
        let active_peer_ids: HashSet<String> = rt
            .ctx
            .active_peers
            .read()
            .await
            .keys()
            .cloned()
            .collect();

        if let Ok(v) = rt.ctx.vault.lock() {
            let q_lower = trimmed.to_lowercase();
            let q_no_at = q_lower.trim_start_matches('@');
            for peer in v.load_all_peers().unwrap_or_default() {
                if peer.profile.peer_id == my_id
                    || peer.profile.is_hidden_relay()
                    || !seen_peers.insert(peer.profile.peer_id.clone())
                {
                    continue;
                }
                let acct = format_account_id(&peer.profile.handle, &peer.profile.peer_id);
                let is_active_now = active_peer_ids.contains(&peer.profile.peer_id);
                let matched = if trimmed.is_empty() {
                    is_active_now
                } else {
                    peer.profile.peer_id == trimmed
                        || acct.to_lowercase().contains(q_no_at)
                        || peer.profile.handle.to_lowercase().contains(q_no_at)
                        || peer.profile.display_name.to_lowercase().contains(&q_lower)
                };
                if matched {
                    results.push(AccountSearchResult {
                        peer_id: peer.profile.peer_id.clone(),
                        account_id: acct,
                        profile: peer.profile,
                        direct_reachable: peer.last_addr.is_some() || is_active_now,
                        relay_hops_available: if is_active_now { 1 } else { 0 },
                        already_paired: true,
                    });
                }
            }
        }

        if trimmed.is_empty() {
            results.retain(|r| !r.already_paired);
        }

        results.sort_by(|a, b| {
            (b.relay_hops_available > 0)
                .cmp(&(a.relay_hops_available > 0))
                .then_with(|| b.profile.updated_at.cmp(&a.profile.updated_at))
        });

        Ok(results)
    }

    /// Connects to a friend cleanly using their `@handle`, `@handle#TAG`, `Peer ID`, or `cymesh://` link.
    pub async fn connect_by_account(&self, identifier: &str) -> Result<PeerSummary> {
        let trimmed = identifier.trim();
        if trimmed.is_empty() {
            return Err(AegisError::PeerNotFound("Empty account identifier".into()));
        }
        if trimmed.starts_with("cymesh://") || trimmed.starts_with("aegis://") {
            return self.add_peer_by_uri(trimmed).await;
        }
        if decode_32_b58(trimmed).is_ok() {
            return self.connect_peer_by_id(trimmed).await;
        }

        let candidates = self.search_accounts(trimmed).await?;
        let lower = trimmed.to_lowercase();
        let no_at = lower.trim_start_matches('@');

        let chosen = candidates
            .iter()
            .find(|c| c.account_id.to_lowercase() == lower || c.account_id.to_lowercase().trim_start_matches('@') == no_at)
            .or_else(|| candidates.iter().find(|c| c.profile.handle.to_lowercase() == lower || c.profile.handle.to_lowercase().trim_start_matches('@') == no_at))
            .or_else(|| candidates.first())
            .ok_or_else(|| AegisError::PeerNotFound(trimmed.to_string()))?;

        self.connect_peer_by_id(&chosen.peer_id).await
    }

    /// Pairs and connects to a remote friend in another city using ONLY their Peer ID via Mesh DHT,
    /// NAT hole punching, or Onion Relay fallback.
    pub async fn connect_peer_by_id(&self, peer_id: &str) -> Result<PeerSummary> {
        let trimmed = peer_id.trim();
        let _ = decode_32_b58(trimmed)?;

        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let my_id = rt.ctx.keys.peer_id();
        if trimmed == my_id {
            return Err(AegisError::InvalidKey("Cannot add yourself".into()));
        }

        // Fast-path: If peer is already actively connected, return immediately
        if rt.ctx.active_peers.read().await.contains_key(trimmed) {
            if let Ok(v) = rt.ctx.vault.lock() {
                if let Ok(peers) = v.load_all_peers() {
                    if let Some(p) = peers.into_iter().find(|p| p.profile.peer_id == trimmed) {
                        return Ok(p);
                    }
                }
            }
        }

        let record_opt = lookup_peer_record_internal(&rt.ctx, trimmed).await?;
        let record = match record_opt {
            Some(rec) => rec,
            None => {
                // Check if peer is already known in local vault (e.g. from LAN discovery beacon or prior pairing)
                if let Ok(v) = rt.ctx.vault.lock() {
                    if let Ok(peers) = v.load_all_peers() {
                        if let Some(p) = peers.into_iter().find(|p| p.profile.peer_id == trimmed) {
                            if let Some(ref addr_str) = p.last_addr {
                                if let Ok(sock) = addr_str.parse::<SocketAddr>() {
                                    connect_and_register_peer(rt.ctx.clone(), sock);
                                }
                            }
                            return Ok(p);
                        }
                    }
                }
                return Err(AegisError::PeerNotFound(trimmed.to_string()));
            }
        };

        let summary = upsert_peer_from_dht_record(&rt.ctx, &record)?;
        let onion_only = rt
            .ctx
            .settings
            .read()
            .map(|s| s.onion_routing_enabled)
            .unwrap_or(false);
        if !onion_only {
            for addr_str in &record.tcp_addrs {
                if let Ok(sock) = addr_str.parse::<SocketAddr>() {
                    connect_and_register_peer(rt.ctx.clone(), sock);
                }
            }
            maybe_initiate_udp_punch(&rt.ctx, &record).await;
        }

        Ok(summary)
    }

    /// Pairs via DHT and triggers Peer-Assisted UDP Hole Punching without TCP dialing.
    pub async fn initiate_udp_punch(&self, peer_id: &str) -> Result<PeerSummary> {
        let trimmed = peer_id.trim();
        let _ = decode_32_b58(trimmed)?;

        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let record = lookup_peer_record_internal(&rt.ctx, trimmed)
            .await?
            .ok_or_else(|| AegisError::PeerNotFound(trimmed.to_string()))?;
        let summary = upsert_peer_from_dht_record(&rt.ctx, &record)?;
        maybe_initiate_udp_punch(&rt.ctx, &record).await;
        Ok(summary)
    }

    /// Direct IP:Port connect (e.g. `192.168.1.25:47833` or `203.0.113.10:47833`)
    pub async fn connect_peer_addr(&self, addr_str: &str) -> Result<PeerSummary> {
        let addr: SocketAddr = addr_str
            .trim()
            .parse()
            .map_err(|_| AegisError::Network(format!("Invalid IP:Port '{addr_str}'")))?;

        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let my_profile = rt
            .ctx
            .own_profile
            .read()
            .map_err(|_| AegisError::Storage("Profile lock".into()))?
            .clone();

        let channel = NoiseChannel::connect(
            addr,
            &rt.ctx.keys.x25519_secret(),
            &rt.ctx.keys.signing_key(),
            &my_profile,
        )
        .await?;

        register_connected_channel(&rt.ctx, channel, Some(addr.to_string())).await
    }

    pub async fn send_message(
        &self,
        peer_id: &str,
        content: String,
        reply_to_id: Option<String>,
        expires_in_secs: Option<u64>,
    ) -> Result<ChatMessage> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let ctx = &rt.ctx;
        let my_id = ctx.keys.peer_id();
        let now = Utc::now().timestamp_millis();
        let msg_id = Uuid::new_v4().to_string();

        let (peer_x_bytes, envelope, ratchet_counter) = {
            let v = ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            let peers = v.load_all_peers()?;
            let peer = peers
                .into_iter()
                .find(|p| p.profile.peer_id == peer_id)
                .ok_or_else(|| AegisError::PeerNotFound(peer_id.to_string()))?;
            let their_x_bytes = decode_32_b58(&peer.profile.x25519_pub)?;
            let their_x_pub = X25519Public::from(their_x_bytes);

            let mut ratchet = match v.load_ratchet_state(peer_id)? {
                Some(r) => r,
                None => DoubleRatchetState::initialize(
                    &ctx.keys.x25519_secret(),
                    &ctx.keys.x25519_public(),
                    &their_x_pub,
                ),
            };
            let (env, counter) = ratchet.encrypt(content.as_bytes())?;
            v.save_ratchet_state(peer_id, &ratchet)?;
            (their_x_bytes, env, counter)
        };

        let expires_at = expires_in_secs.map(|s| now + (s as i64) * 1000);
        let wire_frame = WireFrame::DirectMessage {
            msg_id: msg_id.clone(),
            reply_to_id: reply_to_id.clone(),
            expires_in_secs,
            timestamp: now,
            envelope,
        };

        let onion_forced = ctx
            .settings
            .read()
            .map(|s| s.onion_routing_enabled)
            .unwrap_or(false);
        let mut active_conn = ctx.active_peers.read().await.get(peer_id).cloned();
        if let Some(ref conn) = active_conn {
            if conn.is_udp {
                let udp_alive = ctx
                    .udp_tunnels
                    .read()
                    .ok()
                    .and_then(|m| m.get(peer_id).map(|t| now - t.last_rx_ms <= 15_000))
                    .unwrap_or(false);
                if !udp_alive {
                    cleanup_expired_udp_peer(ctx, peer_id).await;
                    active_conn = None;
                }
            }
        }

        let status = if let (Some(conn), false) = (active_conn, onion_forced) {
            let _ = enqueue_outbox_packet(
                ctx,
                peer_id,
                &peer_x_bytes,
                &msg_id,
                &wire_frame,
                now,
            );
            if conn.tx.send(wire_frame.clone()).await.is_ok() {
                DeliveryStatus::SentDirect
            } else {
                queue_and_route_mesh(ctx, peer_id, &peer_x_bytes, &msg_id, &wire_frame, now).await?
            }
        } else {
            // Opportunistically initiate DHT lookup + UDP hole punch so future messages upgrade to direct speed
            let hole_punch_on = ctx
                .settings
                .read()
                .map(|s| s.hole_punch_enabled)
                .unwrap_or(true);
            if hole_punch_on && !onion_forced {
                let ctx_c = ctx.clone();
                let pid_c = peer_id.to_string();
                tokio::spawn(async move {
                    if let Ok(Some(rec)) = lookup_peer_record_internal(&ctx_c, &pid_c).await {
                        maybe_initiate_udp_punch(&ctx_c, &rec).await;
                    }
                });
            }
            queue_and_route_mesh(ctx, peer_id, &peer_x_bytes, &msg_id, &wire_frame, now).await?
        };

        let chat_msg = ChatMessage {
            id: msg_id,
            chat_id: peer_id.to_string(),
            sender_id: my_id,
            recipient_id: peer_id.to_string(),
            content: content.clone(),
            reply_to_id,
            reactions: Vec::new(),
            timestamp: now,
            expires_at,
            status,
            edited: false,
            ratchet_counter,
        };

        {
            let v = ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            v.save_message(&chat_msg)?;
            let mut peers = v.load_all_peers()?;
            if let Some(p) = peers.iter_mut().find(|p| p.profile.peer_id == peer_id) {
                p.last_message_preview = Some(preview_text(&content));
                p.last_message_at = Some(now);
                v.save_peer(p)?;
                let _ = self.events_tx.send(CoreEvent::PeerUpdated(p.clone()));
            }
        }

        let _ = self
            .events_tx
            .send(CoreEvent::MessageReceived(chat_msg.clone()));
        Ok(chat_msg)
    }

    pub async fn send_typing(&self, peer_id: &str, is_typing: bool) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        if let Some(conn) = rt.ctx.active_peers.read().await.get(peer_id).cloned() {
            let _ = conn.tx.send(WireFrame::Typing { is_typing }).await;
        }
        Ok(())
    }

    pub async fn toggle_reaction(&self, chat_id: &str, msg_id: &str, emoji: String) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let my_id = rt.ctx.keys.peer_id();

        let updated = {
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            let msgs = v.load_messages(chat_id)?;
            let mut target = msgs
                .into_iter()
                .find(|m| m.id == msg_id)
                .ok_or_else(|| AegisError::Storage("Message not found".into()))?;
            if let Some(pos) = target
                .reactions
                .iter()
                .position(|r| r.emoji == emoji && r.sender_id == my_id)
            {
                target.reactions.remove(pos);
            } else {
                target.reactions.push(MessageReaction {
                    emoji: emoji.clone(),
                    sender_id: my_id.clone(),
                });
            }
            v.save_message(&target)?;
            target
        };

        let _ = self.events_tx.send(CoreEvent::MessageUpdated(updated));
        let wire_frame = WireFrame::Reaction {
            msg_id: msg_id.to_string(),
            emoji,
            sender_id: my_id,
        };
        let mut sent_direct = false;
        if let Some(conn) = rt.ctx.active_peers.read().await.get(chat_id).cloned() {
            if conn.tx.send(wire_frame.clone()).await.is_ok() {
                sent_direct = !conn.is_udp;
            }
        }
        if !sent_direct {
            if let Some(target_x) = resolve_peer_x25519(&rt.ctx, chat_id) {
                let now = Utc::now().timestamp_millis();
                let _ = queue_and_route_mesh(
                    &rt.ctx,
                    chat_id,
                    &target_x,
                    &format!("react-{}", Uuid::new_v4()),
                    &wire_frame,
                    now,
                )
                .await;
            }
        }
        Ok(())
    }

    pub async fn edit_message(
        &self,
        chat_id: &str,
        msg_id: &str,
        new_content: String,
    ) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let my_id = rt.ctx.keys.peer_id();

        let updated = {
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            let msgs = v.load_messages(chat_id)?;
            let mut target = msgs
                .into_iter()
                .find(|m| m.id == msg_id && m.sender_id == my_id)
                .ok_or_else(|| AegisError::Storage("Editable message not found".into()))?;
            target.content = new_content.clone();
            target.edited = true;
            v.save_message(&target)?;
            target
        };

        let _ = self.events_tx.send(CoreEvent::MessageUpdated(updated));
        let wire_frame = WireFrame::EditMessage {
            msg_id: msg_id.to_string(),
            new_content,
        };
        let mut sent_direct = false;
        if let Some(conn) = rt.ctx.active_peers.read().await.get(chat_id).cloned() {
            if conn.tx.send(wire_frame.clone()).await.is_ok() {
                sent_direct = !conn.is_udp;
            }
        }
        if !sent_direct {
            if let Some(target_x) = resolve_peer_x25519(&rt.ctx, chat_id) {
                let now = Utc::now().timestamp_millis();
                let _ = queue_and_route_mesh(
                    &rt.ctx,
                    chat_id,
                    &target_x,
                    &format!("edit-{}", Uuid::new_v4()),
                    &wire_frame,
                    now,
                )
                .await;
            }
        }
        Ok(())
    }

    pub async fn delete_message(&self, chat_id: &str, msg_id: &str, for_both: bool) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        {
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            v.delete_message(chat_id, msg_id)?;
        }
        let _ = self.events_tx.send(CoreEvent::MessageDeleted {
            chat_id: chat_id.to_string(),
            message_id: msg_id.to_string(),
        });
        if for_both {
            let wire_frame = WireFrame::DeleteMessage {
                msg_id: msg_id.to_string(),
            };
            let mut sent_direct = false;
            if let Some(conn) = rt.ctx.active_peers.read().await.get(chat_id).cloned() {
                if conn.tx.send(wire_frame.clone()).await.is_ok() {
                    sent_direct = !conn.is_udp;
                }
            }
            if !sent_direct {
                if let Some(target_x) = resolve_peer_x25519(&rt.ctx, chat_id) {
                    let now = Utc::now().timestamp_millis();
                    let _ = queue_and_route_mesh(
                        &rt.ctx,
                        chat_id,
                        &target_x,
                        &format!("del-{}", Uuid::new_v4()),
                        &wire_frame,
                        now,
                    )
                    .await;
                }
            }
        }
        Ok(())
    }

    pub async fn delete_chat(&self, peer_id: &str) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        {
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            v.delete_chat(peer_id)?;
        }
        rt.ctx.active_peers.write().await.remove(peer_id);
        cleanup_expired_udp_peer(&rt.ctx, peer_id).await;
        let _ = self.events_tx.send(CoreEvent::ChatDeleted {
            peer_id: peer_id.to_string(),
        });
        if let Ok(tel) = compute_telemetry(&rt.ctx).await {
            let _ = self.events_tx.send(CoreEvent::NetworkStatus(tel));
        }
        Ok(())
    }

    pub async fn mark_chat_read(&self, peer_id: &str) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let mut updated_peer = None;
        let mut to_ack = Vec::new();
        {
            let v = rt
                .ctx
                .vault
                .lock()
                .map_err(|_| AegisError::Storage("Vault lock".into()))?;
            let mut peers = v.load_all_peers()?;
            if let Some(p) = peers.iter_mut().find(|p| p.profile.peer_id == peer_id) {
                if p.unread_count > 0 {
                    p.unread_count = 0;
                    v.save_peer(p)?;
                    updated_peer = Some(p.clone());
                }
            }
            for mut m in v.load_messages(peer_id)? {
                if m.sender_id == peer_id && m.status != DeliveryStatus::Read {
                    m.status = DeliveryStatus::Read;
                    v.save_message(&m)?;
                    to_ack.push(m.id);
                }
            }
        }
        if let Some(p) = updated_peer {
            let _ = self.events_tx.send(CoreEvent::PeerUpdated(p));
        }
        for msg_id in to_ack {
            send_delivery_receipt_auto(&rt.ctx, peer_id, &msg_id, DeliveryStatus::Read).await;
        }
        Ok(())
    }

    pub async fn set_peer_verified(&self, peer_id: &str, verified: bool) -> Result<PeerSummary> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let v = rt
            .ctx
            .vault
            .lock()
            .map_err(|_| AegisError::Storage("Vault lock".into()))?;
        let mut peers = v.load_all_peers()?;
        let peer = peers
            .iter_mut()
            .find(|p| p.profile.peer_id == peer_id)
            .ok_or_else(|| AegisError::PeerNotFound(peer_id.to_string()))?;
        peer.verified = verified;
        v.save_peer(peer)?;
        let out = peer.clone();
        let _ = self.events_tx.send(CoreEvent::PeerUpdated(out.clone()));
        Ok(out)
    }

    pub async fn set_peer_pinned(&self, peer_id: &str, pinned: bool) -> Result<PeerSummary> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let v = rt
            .ctx
            .vault
            .lock()
            .map_err(|_| AegisError::Storage("Vault lock".into()))?;
        let mut peers = v.load_all_peers()?;
        let peer = peers
            .iter_mut()
            .find(|p| p.profile.peer_id == peer_id)
            .ok_or_else(|| AegisError::PeerNotFound(peer_id.to_string()))?;
        peer.pinned = pinned;
        v.save_peer(peer)?;
        let out = peer.clone();
        let _ = self.events_tx.send(CoreEvent::PeerUpdated(out.clone()));
        Ok(out)
    }

    pub async fn get_network_telemetry(&self) -> Result<NetworkTelemetry> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        compute_telemetry(&rt.ctx).await
    }

    pub async fn trigger_discovery_pulse(&self) -> Result<()> {
        let guard = self.runtime.read().await;
        let rt = guard.as_ref().ok_or(AegisError::VaultLocked)?;
        let stealth = rt.ctx.settings.read().map(|s| s.stealth_mode).unwrap_or(false);
        if !stealth {
            let profile = rt
                .ctx
                .own_profile
                .read()
                .map_err(|_| AegisError::Storage("Profile lock".into()))?
                .clone();
            let beacon = DiscoveryBeacon {
                v: 1,
                profile,
                tcp_port: rt.ctx.tcp_port,
                timestamp: Utc::now().timestamp_millis(),
            };
            if let Ok(bytes) = serde_json::to_vec(&beacon) {
                if let Ok(sock) = tokio::net::UdpSocket::bind("0.0.0.0:0").await {
                    let _ = sock.set_broadcast(true);
                    for ip in enumerate_broadcast_addrs() {
                        let _ = sock
                            .send_to(&bytes, SocketAddr::from((ip, rt.ctx.discovery_port)))
                            .await;
                    }
                    let _ = sock
                        .send_to(
                            &bytes,
                            SocketAddr::from((Ipv4Addr::LOCALHOST, rt.ctx.discovery_port)),
                        )
                        .await;
                }
            }
        }

        let seeds = rt
            .ctx
            .settings
            .read()
            .map(|s| s.bootstrap_peers.clone())
            .unwrap_or_default();
        trigger_mesh_bootstrap(&rt.ctx, &seeds).await;
        announce_own_dht_record(&rt.ctx).await;

        // Proactively retry dialing any known contacts currently marked offline
        let connected: HashSet<String> = rt.ctx.active_peers.read().await.keys().cloned().collect();
        if let Ok(v) = rt.ctx.vault.lock() {
            if let Ok(peers) = v.load_all_peers() {
                for p in peers {
                    if !connected.contains(&p.profile.peer_id) {
                        if let Some(addr_str) = p.last_addr {
                            if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                                connect_and_register_peer(rt.ctx.clone(), addr);
                            }
                        }
                    }
                }
            }
        }
        if let Ok(tel) = compute_telemetry(&rt.ctx).await {
            let _ = rt.ctx.events_tx.send(CoreEvent::NetworkStatus(tel));
        }
        Ok(())
    }
}

fn enqueue_outbox_packet(
    ctx: &Arc<NodeContext>,
    recipient_id: &str,
    recipient_x_pub: &[u8; 32],
    msg_id: &str,
    frame: &WireFrame,
    now: i64,
) -> Result<SealedMeshPacket> {
    let my_id = ctx.keys.peer_id();
    let own_profile = ctx.own_profile.read().ok().map(|p| p.clone());
    let inner = SealedInnerPayload {
        sender_peer_id: my_id,
        payload_json: serde_json::to_vec(frame)?,
        sender_profile: own_profile,
    };
    let sealed = seal_for_recipient(
        msg_id.to_string(),
        recipient_id,
        recipient_x_pub,
        &inner,
        4,
        now + 7 * 24 * 3600 * 1000,
    )?;

    if matches!(frame, WireFrame::DirectMessage { .. }) {
        let v = ctx
            .vault
            .lock()
            .map_err(|_| AegisError::Storage("Vault lock".into()))?;
        v.enqueue_outbox(&OutboxItem {
            msg_id: msg_id.to_string(),
            recipient_id: recipient_id.to_string(),
            sealed: sealed.clone(),
            created_at: now,
        })?;
    }
    Ok(sealed)
}

async fn cleanup_expired_udp_peer(ctx: &Arc<NodeContext>, peer_id: &str) {
    if let Ok(mut w) = ctx.udp_tunnels.write() {
        w.remove(peer_id);
    }
    let removed = {
        let mut map = ctx.active_peers.write().await;
        if let Some(conn) = map.get(peer_id) {
            if conn.is_udp {
                map.remove(peer_id);
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if removed {
        let has_relays = ctx
            .active_peers
            .read()
            .await
            .values()
            .any(|c| c.accepts_relay);
        let fallback_route = if has_relays {
            ConnectionRoute::MeshRelay
        } else {
            ConnectionRoute::Offline
        };
        if let Ok(v) = ctx.vault.lock() {
            if let Ok(peers) = v.load_all_peers() {
                if let Some(mut p) = peers.into_iter().find(|p| p.profile.peer_id == peer_id) {
                    p.route = fallback_route;
                    let _ = v.save_peer(&p);
                    let _ = ctx.events_tx.send(CoreEvent::PeerUpdated(p));
                }
            }
        }
    }
}

fn handle_delivery_receipt(
    ctx: &Arc<NodeContext>,
    peer_id: &str,
    msg_id: &str,
    status: DeliveryStatus,
) {
    let mut updated_msg = None;
    if let Ok(v) = ctx.vault.lock() {
        let _ = v.remove_outbox(msg_id);
        if let Ok(msgs) = v.load_messages(peer_id) {
            if let Some(mut target) = msgs.into_iter().find(|m| m.id == msg_id) {
                if target.status != DeliveryStatus::Read || status == DeliveryStatus::Read {
                    target.status = status;
                    if v.save_message(&target).is_ok() {
                        updated_msg = Some(target);
                    }
                }
            }
        }
    }
    if let Some(m) = updated_msg {
        let _ = ctx.events_tx.send(CoreEvent::MessageUpdated(m));
    }
}

async fn send_delivery_receipt_auto(
    ctx: &Arc<NodeContext>,
    peer_id: &str,
    msg_id: &str,
    status: DeliveryStatus,
) {
    if let Some(direct_conn) = ctx.active_peers.read().await.get(peer_id).cloned() {
        let _ = direct_conn
            .tx
            .send(WireFrame::DeliveryReceipt {
                msg_id: msg_id.to_string(),
                status,
            })
            .await;
    } else if let Some(sender_x) = resolve_peer_x25519(ctx, peer_id) {
        let frame = WireFrame::DeliveryReceipt {
            msg_id: msg_id.to_string(),
            status,
        };
        let now = Utc::now().timestamp_millis();
        let _ = queue_and_route_mesh(
            ctx,
            peer_id,
            &sender_x,
            &format!("receipt-{}", Uuid::new_v4()),
            &frame,
            now,
        )
        .await;
    }
}

/// Enqueues message in local E2EE Outbox and routes it across the Mesh Overlay:
/// 1) Multi-hop Blinded Onion Circuit (`OnionRelay`) when relays are active,
/// 2) Encrypted DHT Store-and-Forward Mailbox (`SealedRelay`) for offline / cross-city sync.
async fn queue_and_route_mesh(
    ctx: &Arc<NodeContext>,
    recipient_id: &str,
    recipient_x_pub: &[u8; 32],
    msg_id: &str,
    frame: &WireFrame,
    now: i64,
) -> Result<DeliveryStatus> {
    let sealed = enqueue_outbox_packet(ctx, recipient_id, recipient_x_pub, msg_id, frame, now)?;

    let mesh_enabled = ctx
        .settings
        .read()
        .map(|s| s.mesh_relay_enabled)
        .unwrap_or(true);
    if !mesh_enabled {
        return Ok(DeliveryStatus::QueuedOffline);
    }

    let active_snapshot: Vec<(String, ActivePeerConn)> = ctx
        .active_peers
        .read()
        .await
        .iter()
        .map(|(id, c)| (id.clone(), c.clone()))
        .collect();

    let mut relay_neighbors: Vec<(String, ActivePeerConn)> = active_snapshot
        .iter()
        .filter(|(id, c)| id != recipient_id && c.accepts_relay)
        .cloned()
        .collect();

    if relay_neighbors.is_empty() {
        return Ok(DeliveryStatus::QueuedOffline);
    }

    // Prefer an exit relay advertised by the recipient in their SignedDhtRecord
    let target_record = ctx
        .dht_records
        .read()
        .ok()
        .and_then(|s| s.get(recipient_id, now));

    let mut sent_any = false;
    if let Some(ref rec) = target_record {
        if let Some(exit_hint) = rec.relay_hints.first() {
            if let Ok(exit_x) = decode_32_b58(&exit_hint.x25519_pub) {
                // Choose a distinct entry relay if we have one (true 2-hop blinded onion circuit!),
                // otherwise send 1-hop directly to the exit relay if directly connected.
                if let Some((entry_id, entry_conn)) = relay_neighbors
                    .iter()
                    .find(|(id, _)| id != &exit_hint.peer_id)
                {
                    let hops = vec![
                        (entry_id.clone(), entry_conn.x25519_pub),
                        (exit_hint.peer_id.clone(), exit_x),
                    ];
                    if let Ok(onion) = build_onion_packet(&hops, recipient_id, sealed.clone()) {
                        if entry_conn.tx.send(WireFrame::OnionRelay(onion)).await.is_ok() {
                            ctx.onion_routed_count.fetch_add(1, Ordering::Relaxed);
                            sent_any = true;
                        }
                    }
                } else if let Some((_, direct_exit)) = relay_neighbors
                    .iter()
                    .find(|(id, _)| id == &exit_hint.peer_id)
                {
                    let hops = vec![(exit_hint.peer_id.clone(), exit_x)];
                    if let Ok(onion) = build_onion_packet(&hops, recipient_id, sealed.clone()) {
                        if direct_exit.tx.send(WireFrame::OnionRelay(onion)).await.is_ok() {
                            ctx.onion_routed_count.fetch_add(1, Ordering::Relaxed);
                            sent_any = true;
                        }
                    }
                }
            }
        }
    }

    // If no exit relay hint matched yet, build a blinded 2-hop (or 1-hop) onion path via local relays
    if !sent_any {
        relay_neighbors.sort_by_key(|(id, _)| {
            crate::dht::xor_distance(
                &crate::dht::node_id_from_peer_id(id),
                &crate::dht::node_id_from_peer_id(recipient_id),
            )
        });
        let hops: Vec<(String, [u8; 32])> = if relay_neighbors.len() >= 2 {
            vec![
                (relay_neighbors[1].0.clone(), relay_neighbors[1].1.x25519_pub),
                (relay_neighbors[0].0.clone(), relay_neighbors[0].1.x25519_pub),
            ]
        } else {
            vec![(relay_neighbors[0].0.clone(), relay_neighbors[0].1.x25519_pub)]
        };
        let entry_tx = if relay_neighbors.len() >= 2 {
            &relay_neighbors[1].1.tx
        } else {
            &relay_neighbors[0].1.tx
        };
        if let Ok(onion) = build_onion_packet(&hops, recipient_id, sealed.clone()) {
            if entry_tx.send(WireFrame::OnionRelay(onion)).await.is_ok() {
                ctx.onion_routed_count.fetch_add(1, Ordering::Relaxed);
                sent_any = true;
            }
        }
    }

    // Also replicate SealedRelay to closest DHT neighbor(s) for offline mailbox availability
    for (_, conn) in relay_neighbors.into_iter().take(2) {
        if conn
            .tx
            .send(WireFrame::SealedRelay(sealed.clone()))
            .await
            .is_ok()
        {
            sent_any = true;
        }
    }

    if sent_any {
        Ok(DeliveryStatus::RelayedMesh)
    } else {
        Ok(DeliveryStatus::QueuedOffline)
    }
}

fn build_own_dht_record(ctx: &Arc<NodeContext>, active_relays: Vec<RelayHint>) -> Option<SignedDhtRecord> {
    let now = Utc::now().timestamp_millis();
    let stealth = ctx.settings.read().map(|s| s.stealth_mode).unwrap_or(false);
    let accepts_relay = ctx
        .settings
        .read()
        .map(|s| s.mesh_relay_enabled)
        .unwrap_or(true);
    let profile = ctx.own_profile.read().ok().map(|p| p.clone());

    let mut tcp_addrs = Vec::new();
    for ip in enumerate_local_ipv4s() {
        let a = format!("{ip}:{}", ctx.tcp_port);
        if !tcp_addrs.contains(&a) {
            tcp_addrs.push(a);
        }
    }
    if let Ok(obs) = ctx.observed_external_addr.read() {
        if let Some(ext) = obs.as_ref() {
            if let Ok(sock) = ext.parse::<SocketAddr>() {
                let candidate = SocketAddr::new(sock.ip(), ctx.tcp_port).to_string();
                if !tcp_addrs.contains(&candidate) {
                    tcp_addrs.push(candidate);
                }
            }
        }
    }
    if tcp_addrs.is_empty() {
        tcp_addrs.push(format!("127.0.0.1:{}", ctx.tcp_port));
    }

    let udp_punch_addr = ctx
        .observed_external_addr
        .read()
        .ok()
        .and_then(|o| o.clone())
        .or_else(|| tcp_addrs.first().and_then(|a| a.parse::<SocketAddr>().ok()).map(|s| SocketAddr::new(s.ip(), ctx.udp_port).to_string()));

    let seq = ctx.dht_seq.fetch_add(1, Ordering::Relaxed);
    let mut rec = SignedDhtRecord {
        peer_id: ctx.keys.peer_id(),
        x25519_pub: ctx.keys.x25519_pub_b58(),
        tcp_addrs,
        udp_punch_addr,
        relay_hints: active_relays,
        accepts_relay,
        profile: if stealth { None } else { profile },
        seq,
        published_at: now,
        expires_at: now + DHT_RECORD_TTL_MS,
        signature: String::new(),
    };
    rec.sign(&ctx.keys.signing_key());
    Some(rec)
}

async fn announce_own_dht_record(ctx: &Arc<NodeContext>) {
    let dht_on = ctx.settings.read().map(|s| s.dht_enabled).unwrap_or(true);
    if !dht_on {
        return;
    }

    let peers_snapshot: Vec<(String, ActivePeerConn)> = ctx
        .active_peers
        .read()
        .await
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let relay_hints: Vec<RelayHint> = peers_snapshot
        .iter()
        .filter(|(_, c)| c.accepts_relay)
        .take(4)
        .filter_map(|(id, c)| {
            let mut addrs = Vec::new();
            if let Some(a) = &c.remote_addr {
                addrs.push(a.clone());
            } else if let Ok(rt) = ctx.routing_table.read() {
                if let Some(contact) = rt.get(id) {
                    addrs = contact.tcp_addrs;
                }
            }
            Some(RelayHint {
                peer_id: id.clone(),
                x25519_pub: bs58::encode(c.x25519_pub).into_string(),
                addrs,
            })
        })
        .collect();

    let Some(rec) = build_own_dht_record(ctx, relay_hints) else {
        return;
    };

    let now = Utc::now().timestamp_millis();
    if let Ok(mut store) = ctx.dht_records.write() {
        let _ = store.upsert(rec.clone(), now);
    }

    for (_, conn) in peers_snapshot {
        let _ = conn.tx.send(WireFrame::DhtAnnounceRecord(rec.clone())).await;
    }
}

async fn broadcast_dht_withdrawal(ctx: &Arc<NodeContext>) {
    let my_id = ctx.keys.peer_id();
    let sig = bs58::encode(ctx.keys.signing_key().sign(b"CYMESH_DHT_WITHDRAW").to_bytes()).into_string();
    let frame = WireFrame::DhtWithdrawRecord {
        peer_id: my_id.clone(),
        signature: sig,
    };
    if let Ok(mut store) = ctx.dht_records.write() {
        store.remove(&my_id);
    }
    let peers_snapshot: Vec<mpsc::Sender<WireFrame>> = ctx
        .active_peers
        .read()
        .await
        .values()
        .map(|c| c.tx.clone())
        .collect();
    for tx in peers_snapshot {
        let _ = tx.send(frame.clone()).await;
    }
}

async fn trigger_mesh_bootstrap(ctx: &Arc<NodeContext>, extra_seeds: &[String]) {
    let connected_addrs: HashSet<String> = ctx
        .active_peers
        .read()
        .await
        .values()
        .filter_map(|c| c.remote_addr.clone())
        .collect();

    let mut addrs_to_dial: Vec<SocketAddr> = Vec::new();
    for seed in extra_seeds {
        if let Ok(sock) = seed.trim().parse::<SocketAddr>() {
            if !connected_addrs.contains(&sock.to_string()) {
                addrs_to_dial.push(sock);
            }
        }
    }

    let onion_only = ctx
        .settings
        .read()
        .map(|s| s.onion_routing_enabled)
        .unwrap_or(false);
    if !onion_only {
        if let Ok(table) = ctx.routing_table.read() {
            for c in table.all_contacts().into_iter().take(16) {
                for a in c.tcp_addrs {
                    if let Ok(sock) = a.parse::<SocketAddr>() {
                        if !connected_addrs.contains(&sock.to_string()) && !addrs_to_dial.contains(&sock) {
                            addrs_to_dial.push(sock);
                        }
                    }
                }
            }
        }
    }

    for sock in addrs_to_dial.into_iter().take(16) {
        connect_and_register_peer(ctx.clone(), sock);
    }
}

async fn lookup_peer_record_internal(
    ctx: &Arc<NodeContext>,
    target_peer_id: &str,
) -> Result<Option<SignedDhtRecord>> {
    let now = Utc::now().timestamp_millis();
    if let Ok(store) = ctx.dht_records.read() {
        if let Some(rec) = store.get(target_peer_id, now) {
            return Ok(Some(rec));
        }
    }

    let senders: Vec<mpsc::Sender<WireFrame>> = ctx
        .active_peers
        .read()
        .await
        .values()
        .map(|c| c.tx.clone())
        .collect();

    if senders.is_empty() {
        return Ok(None);
    }

    let req_id = Uuid::new_v4().to_string();
    let (wait_tx, wait_rx) = oneshot::channel();
    if let Ok(mut pending) = ctx.pending_dht_queries.lock() {
        pending.insert(req_id.clone(), (target_peer_id.to_string(), wait_tx));
    }

    for tx in senders {
        let _ = tx
            .send(WireFrame::DhtQueryRecord {
                req_id: req_id.clone(),
                target_peer_id: target_peer_id.to_string(),
            })
            .await;
    }

    let found = match tokio::time::timeout(Duration::from_millis(1800), wait_rx).await {
        Ok(Ok(Some(rec))) => Some(rec),
        _ => ctx
            .dht_records
            .read()
            .ok()
            .and_then(|s| s.get(target_peer_id, Utc::now().timestamp_millis())),
    };

    if let Ok(mut pending) = ctx.pending_dht_queries.lock() {
        pending.remove(&req_id);
    }
    Ok(found)
}

fn upsert_peer_from_dht_record(
    ctx: &Arc<NodeContext>,
    rec: &SignedDhtRecord,
) -> Result<PeerSummary> {
    let my_id = ctx.keys.peer_id();
    let profile = if let Some(ref prof) = rec.profile {
        prof.clone()
    } else {
        SignedProfileCard {
            peer_id: rec.peer_id.clone(),
            x25519_pub: rec.x25519_pub.clone(),
            display_name: format!("Node {}", &rec.peer_id[..6.min(rec.peer_id.len())]),
            handle: "@node".to_string(),
            bio: "Verified Cymesh Account".to_string(),
            role_badge: "Cymesh".to_string(),
            avatar_color: "#00F0FF".to_string(),
            avatar_data: None,
            status: PresenceStatus::Online,
            updated_at: rec.published_at,
            signature: rec.signature.clone(),
        }
    };

    let addr_hint = rec.tcp_addrs.first().cloned();
    // Skip re-verifying synthetic fallback card if profile was hidden (stealth DHT)
    let mut summary = if rec.profile.is_some() {
        upsert_peer_in_vault(&ctx.vault, &my_id, profile, addr_hint)?
    } else {
        let (safety_number, safety_emojis) = compute_safety_verification(&my_id, &rec.peer_id)?;
        let fingerprint = format_fingerprint(&rec.peer_id);
        let account_id = format_account_id(&profile.handle, &rec.peer_id);
        let s = PeerSummary {
            profile,
            account_id,
            fingerprint,
            route: if rec.relay_hints.is_empty() {
                ConnectionRoute::Offline
            } else {
                ConnectionRoute::MeshRelay
            },
            latency_ms: None,
            verified: false,
            safety_number,
            safety_emojis,
            last_addr: addr_hint,
            last_seen: Utc::now().timestamp_millis(),
            unread_count: 0,
            last_message_preview: None,
            last_message_at: None,
            pinned: false,
        };
        let v = ctx
            .vault
            .lock()
            .map_err(|_| AegisError::Storage("Vault lock".into()))?;
        if let Some(existing) = v
            .load_all_peers()?
            .into_iter()
            .find(|p| p.profile.peer_id == rec.peer_id)
        {
            existing
        } else {
            v.save_peer(&s)?;
            s
        }
    };

    if summary.route == ConnectionRoute::Offline && (!rec.relay_hints.is_empty() || rec.accepts_relay) {
        summary.route = ConnectionRoute::MeshRelay;
        if let Ok(v) = ctx.vault.lock() {
            let _ = v.save_peer(&summary);
        }
    }

    let _ = ctx.events_tx.send(CoreEvent::PeerUpdated(summary.clone()));
    Ok(summary)
}

fn spawn_tcp_accept_loop(ctx: Arc<NodeContext>, listener: TcpListener) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Ok((stream, remote_addr)) = listener.accept().await else {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            };
            let ctx_c = ctx.clone();
            tokio::spawn(async move {
                let profile_snap = ctx_c.own_profile.read().map(|p| p.clone()).ok();
                let Some(my_prof) = profile_snap else {
                    return;
                };
                match NoiseChannel::accept(
                    stream,
                    &ctx_c.keys.x25519_secret(),
                    &ctx_c.keys.signing_key(),
                    &my_prof,
                )
                .await
                {
                    Ok(channel) => {
                        let _ = register_connected_channel(
                            &ctx_c,
                            channel,
                            Some(remote_addr.to_string()),
                        )
                        .await;
                    }
                    Err(e) => {
                        tracing::debug!("Incoming Noise handshake rejected from {remote_addr}: {e}");
                    }
                }
            });
        }
    })
}

fn spawn_udp_mesh_loop(ctx: Arc<NodeContext>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];
        let my_id = ctx.keys.peer_id();
        loop {
            let Ok((len, src_addr)) = ctx.udp_socket.recv_from(&mut buf).await else {
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            };
            let Ok(pkt) = serde_json::from_slice::<UdpMeshPacket>(&buf[..len]) else {
                continue;
            };

            match pkt {
                UdpMeshPacket::EchoReq { from_peer_id, nonce } => {
                    if from_peer_id != my_id {
                        let resp = UdpMeshPacket::EchoResp {
                            from_peer_id: my_id.clone(),
                            nonce,
                            observed_addr: src_addr.to_string(),
                        };
                        if let Ok(bytes) = serde_json::to_vec(&resp) {
                            let _ = ctx.udp_socket.send_to(&bytes, src_addr).await;
                        }
                    }
                }
                UdpMeshPacket::EchoResp {
                    nonce,
                    observed_addr,
                    ..
                } => {
                    let valid = ctx
                        .pending_echo_nonces
                        .lock()
                        .ok()
                        .and_then(|mut m| m.remove(&nonce))
                        .is_some();
                    if valid {
                        if let Ok(mut w) = ctx.observed_external_addr.write() {
                            *w = Some(observed_addr);
                        }
                    }
                }
                UdpMeshPacket::PunchHello {
                    from_peer_id,
                    to_peer_id,
                    punch_id,
                    eph_x_pub,
                    sig,
                } => {
                    if to_peer_id != my_id {
                        continue;
                    }
                    if verify_punch_handshake(
                        "HELLO",
                        &punch_id,
                        &from_peer_id,
                        &to_peer_id,
                        &eph_x_pub,
                        &sig,
                    )
                    .is_err()
                    {
                        continue;
                    }

                    let mut completed_cipher: Option<(UdpTunnelCipher, SocketAddr)> = None;
                    let mut reply_eph_pub: Option<String> = None;

                    if let Ok(mut pending) = ctx.pending_punches.lock() {
                        if let Some(p_state) = pending.get(&punch_id) {
                            if let Ok(their_eph_arr) = decode_32_b58(&eph_x_pub) {
                                let cipher = UdpTunnelCipher::derive(
                                    &my_id,
                                    &from_peer_id,
                                    &ctx.keys.x25519_secret(),
                                    &X25519Public::from(p_state.target_static_x),
                                    &p_state.my_eph_secret,
                                    &X25519Public::from(their_eph_arr),
                                    &punch_id,
                                );
                                reply_eph_pub = Some(p_state.my_eph_pub_b58.clone());
                                completed_cipher = Some((cipher, src_addr));
                            }
                            pending.remove(&punch_id);
                        }
                    }

                    if let (Some((cipher, target_addr)), Some(my_eph_pub)) =
                        (completed_cipher, reply_eph_pub)
                    {
                        let ack_sig = sign_punch_handshake(
                            &ctx.keys,
                            "ACK",
                            &punch_id,
                            &from_peer_id,
                            &my_eph_pub,
                        );
                        let ack = UdpMeshPacket::PunchAck {
                            from_peer_id: my_id.clone(),
                            to_peer_id: from_peer_id.clone(),
                            punch_id,
                            eph_x_pub: my_eph_pub,
                            sig: ack_sig,
                        };
                        if let Ok(bytes) = serde_json::to_vec(&ack) {
                            let _ = ctx.udp_socket.send_to(&bytes, target_addr).await;
                        }
                        activate_udp_tunnel(&ctx, &from_peer_id, cipher, target_addr).await;
                    }
                }
                UdpMeshPacket::PunchAck {
                    from_peer_id,
                    to_peer_id,
                    punch_id,
                    eph_x_pub,
                    sig,
                } => {
                    if to_peer_id != my_id {
                        continue;
                    }
                    if verify_punch_handshake(
                        "ACK",
                        &punch_id,
                        &from_peer_id,
                        &to_peer_id,
                        &eph_x_pub,
                        &sig,
                    )
                    .is_err()
                    {
                        continue;
                    }

                    let mut completed: Option<UdpTunnelCipher> = None;
                    if let Ok(mut pending) = ctx.pending_punches.lock() {
                        if let Some(p_state) = pending.remove(&punch_id) {
                            if let Ok(their_eph_arr) = decode_32_b58(&eph_x_pub) {
                                let cipher = UdpTunnelCipher::derive(
                                    &my_id,
                                    &from_peer_id,
                                    &ctx.keys.x25519_secret(),
                                    &X25519Public::from(p_state.target_static_x),
                                    &p_state.my_eph_secret,
                                    &X25519Public::from(their_eph_arr),
                                    &punch_id,
                                );
                                completed = Some(cipher);
                            }
                        }
                    }
                    if let Some(cipher) = completed {
                        activate_udp_tunnel(&ctx, &from_peer_id, cipher, src_addr).await;
                    }
                }
                UdpMeshPacket::TunnelData {
                    from_peer_id,
                    nonce,
                    ciphertext,
                } => {
                    let cipher_opt = ctx
                        .udp_tunnels
                        .read()
                        .ok()
                        .and_then(|m| m.get(&from_peer_id).map(|t| t.cipher.clone()));
                    let Some(cipher) = cipher_opt else {
                        continue;
                    };
                    let Ok(plain) = cipher.decrypt(&nonce, &ciphertext) else {
                        continue;
                    };
                    if let Ok(mut w) = ctx.udp_tunnels.write() {
                        if let Some(t) = w.get_mut(&from_peer_id) {
                            t.remote_udp_addr = src_addr;
                            t.last_rx_ms = Utc::now().timestamp_millis();
                        }
                    }
                    let Ok(frame) = serde_json::from_slice::<WireFrame>(&plain) else {
                        continue;
                    };
                    let tx_opt = ctx
                        .active_peers
                        .read()
                        .await
                        .get(&from_peer_id)
                        .map(|c| c.tx.clone());
                    if let Some(peer_tx) = tx_opt {
                        handle_incoming_frame(&ctx, frame, &from_peer_id, &peer_tx).await;
                    }
                }
                UdpMeshPacket::TunnelKeepalive { from_peer_id, .. } => {
                    if let Ok(mut w) = ctx.udp_tunnels.write() {
                        if let Some(t) = w.get_mut(&from_peer_id) {
                            t.remote_udp_addr = src_addr;
                            t.last_rx_ms = Utc::now().timestamp_millis();
                        }
                    }
                }
            }
        }
    })
}

async fn activate_udp_tunnel(
    ctx: &Arc<NodeContext>,
    peer_id: &str,
    cipher: UdpTunnelCipher,
    remote_udp_addr: SocketAddr,
) {
    let now = Utc::now().timestamp_millis();
    let already_new = ctx
        .udp_tunnels
        .read()
        .ok()
        .map(|m| m.contains_key(peer_id))
        .unwrap_or(false);
    if !already_new {
        ctx.nat_punch_successes.fetch_add(1, Ordering::Relaxed);
    }

    if let Ok(mut w) = ctx.udp_tunnels.write() {
        w.insert(
            peer_id.to_string(),
            ActiveUdpTunnel {
                cipher: cipher.clone(),
                remote_udp_addr,
                last_rx_ms: now,
            },
        );
    }

    // Do not replace an active Noise TCP channel if one already exists
    let has_active_tcp = ctx.active_peers.read().await.contains_key(peer_id);
    if has_active_tcp {
        return;
    }

    let x_bytes = resolve_peer_x25519(ctx, peer_id).unwrap_or([0u8; 32]);
    let (tx, mut rx) = mpsc::channel::<WireFrame>(256);
    let route = if is_private_or_local_ip(remote_udp_addr.ip()) {
        ConnectionRoute::LanDirect
    } else {
        ConnectionRoute::WanDirect
    };

    {
        let mut map = ctx.active_peers.write().await;
        map.insert(
            peer_id.to_string(),
            ActivePeerConn {
                tx: tx.clone(),
                route,
                remote_addr: Some(remote_udp_addr.to_string()),
                udp_port: remote_udp_addr.port(),
                accepts_relay: true,
                x25519_pub: x_bytes,
                is_udp: true,
            },
        );
    }

    if let Ok(v) = ctx.vault.lock() {
        if let Ok(peers) = v.load_all_peers() {
            if let Some(mut p) = peers.into_iter().find(|p| p.profile.peer_id == peer_id) {
                p.route = route;
                p.last_seen = now;
                let _ = v.save_peer(&p);
                let _ = ctx.events_tx.send(CoreEvent::PeerUpdated(p));
            }
        }
    }

    let ctx_w = ctx.clone();
    let peer_w = peer_id.to_string();
    let writer_handle = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            let Ok(plain) = serde_json::to_vec(&frame) else {
                continue;
            };
            let current_tunnel = ctx_w
                .udp_tunnels
                .read()
                .ok()
                .and_then(|m| m.get(&peer_w).cloned());
            let Some(tunnel) = current_tunnel else {
                break;
            };
            let Ok((nonce, ciphertext)) = tunnel.cipher.encrypt(&plain) else {
                continue;
            };
            let pkt = UdpMeshPacket::TunnelData {
                from_peer_id: ctx_w.keys.peer_id(),
                nonce,
                ciphertext,
            };
            if let Ok(bytes) = serde_json::to_vec(&pkt) {
                let _ = ctx_w.udp_socket.send_to(&bytes, tunnel.remote_udp_addr).await;
            }
        }
    });

    if let Ok(mut list) = ctx.peer_handles.lock() {
        list.retain(|h| !h.is_finished());
        list.push(writer_handle.abort_handle());
    }

    flush_outbox_and_mesh_for_peer(ctx, peer_id, &tx).await;
    let prof_opt = ctx.own_profile.read().ok().map(|p| p.clone());
    if let Some(prof) = prof_opt {
        let _ = tx.send(WireFrame::ProfileSync(prof)).await;
    }
}

async fn maybe_initiate_udp_punch(ctx: &Arc<NodeContext>, record: &SignedDhtRecord) {
    let punch_on = ctx
        .settings
        .read()
        .map(|s| s.hole_punch_enabled)
        .unwrap_or(true);
    if !punch_on {
        return;
    }
    let my_id = ctx.keys.peer_id();
    if record.peer_id == my_id || ctx.active_peers.read().await.contains_key(&record.peer_id) {
        return;
    }
    let Some(ref target_udp_str) = record.udp_punch_addr else {
        return;
    };
    let Ok(target_udp_addr) = target_udp_str.parse::<SocketAddr>() else {
        return;
    };
    let Ok(target_static_x) = decode_32_b58(&record.x25519_pub) else {
        return;
    };

    let recently_pending = ctx
        .pending_punches
        .lock()
        .ok()
        .map(|m| m.values().any(|p| p.target_peer_id == record.peer_id))
        .unwrap_or(false);
    if recently_pending {
        return;
    }

    let my_udp_addr = ctx
        .observed_external_addr
        .read()
        .ok()
        .and_then(|o| o.clone())
        .unwrap_or_else(|| {
            let ip = enumerate_local_ipv4s()
                .into_iter()
                .next()
                .unwrap_or(Ipv4Addr::LOCALHOST);
            format!("{ip}:{}", ctx.udp_port)
        });

    let eph_secret = X25519Static::random_from_rng(rand::rngs::OsRng);
    let eph_pub_b58 = bs58::encode(X25519Public::from(&eph_secret).as_bytes()).into_string();
    let punch_id = Uuid::new_v4().to_string();

    if let Ok(mut pending) = ctx.pending_punches.lock() {
        pending.insert(
            punch_id.clone(),
            PendingPunchState {
                target_peer_id: record.peer_id.clone(),
                target_static_x,
                my_eph_secret: eph_secret,
                my_eph_pub_b58: eph_pub_b58.clone(),
                target_udp_addr: Some(target_udp_addr),
                created_at: Utc::now().timestamp_millis(),
            },
        );
    }

    let coord = WireFrame::PunchCoordinate {
        punch_id: punch_id.clone(),
        initiator_peer_id: my_id.clone(),
        initiator_x25519_pub: eph_pub_b58.clone(),
        initiator_udp_addr: my_udp_addr,
        target_peer_id: record.peer_id.clone(),
    };

    // Route PunchCoordinate via active relays (prefer target's relay hints)
    let active_map = ctx.active_peers.read().await;
    let mut forwarded = false;
    for hint in &record.relay_hints {
        if let Some(conn) = active_map.get(&hint.peer_id) {
            if conn.tx.send(coord.clone()).await.is_ok() {
                forwarded = true;
            }
        }
    }
    if !forwarded {
        for (id, conn) in active_map.iter() {
            if id != &record.peer_id && conn.accepts_relay {
                let _ = conn.tx.send(coord.clone()).await;
            }
        }
    }
    drop(active_map);

    // Fire UDP punch hello bursts immediately toward target's advertised UDP endpoint
    let sig = sign_punch_handshake(&ctx.keys, "HELLO", &punch_id, &record.peer_id, &eph_pub_b58);
    let hello = UdpMeshPacket::PunchHello {
        from_peer_id: my_id,
        to_peer_id: record.peer_id.clone(),
        punch_id,
        eph_x_pub: eph_pub_b58,
        sig,
    };
    if let Ok(bytes) = serde_json::to_vec(&hello) {
        let udp_sock = ctx.udp_socket.clone();
        tokio::spawn(async move {
            for _ in 0..8 {
                let _ = udp_sock.send_to(&bytes, target_udp_addr).await;
                tokio::time::sleep(Duration::from_millis(85)).await;
            }
        });
    }
}

fn spawn_discovery_loop(ctx: Arc<NodeContext>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Ok(udp) = bind_discovery_socket(ctx.discovery_port) else {
            return;
        };
        let my_id = ctx.keys.peer_id();
        let mut buf = vec![0u8; 65535];
        let mut beacon_interval = tokio::time::interval(Duration::from_secs(3));
        let mut dial_cooldowns: HashMap<String, i64> = HashMap::new();

        loop {
            tokio::select! {
                _ = beacon_interval.tick() => {
                    let stealth = ctx.settings.read().map(|s| s.stealth_mode).unwrap_or(false);
                    if !stealth {
                        let prof_opt = ctx.own_profile.read().ok().map(|p| p.clone());
                        if let Some(prof) = prof_opt {
                            let beacon = DiscoveryBeacon {
                                v: 1,
                                profile: prof,
                                tcp_port: ctx.tcp_port,
                                timestamp: Utc::now().timestamp_millis(),
                            };
                            if let Ok(bytes) = serde_json::to_vec(&beacon) {
                                for bcast_ip in enumerate_broadcast_addrs() {
                                    let _ = udp.send_to(&bytes, SocketAddr::from((bcast_ip, ctx.discovery_port))).await;
                                }
                                let _ = udp.send_to(&bytes, SocketAddr::from((Ipv4Addr::LOCALHOST, ctx.discovery_port))).await;
                            }
                        }
                    }
                }
                recv_res = udp.recv_from(&mut buf) => {
                    let Ok((len, src_addr)) = recv_res else {
                        continue;
                    };
                    let Ok(beacon) = serde_json::from_slice::<DiscoveryBeacon>(&buf[..len]) else {
                        continue;
                    };
                    if beacon.profile.peer_id == my_id || beacon.profile.verify().is_err() {
                        continue;
                    }
                    let peer_tcp_addr = SocketAddr::new(src_addr.ip(), beacon.tcp_port);
                    let peer_id = beacon.profile.peer_id.clone();

                    let already_connected = ctx.active_peers.read().await.contains_key(&peer_id);
                    if !beacon.profile.is_hidden_relay() {
                        if let Ok(mut summary) = upsert_peer_in_vault(&ctx.vault, &my_id, beacon.profile, Some(peer_tcp_addr.to_string())) {
                            summary.route = ConnectionRoute::LanDirect;
                            summary.latency_ms = ctx.peer_latencies.read().ok().and_then(|l| l.get(&peer_id).copied());
                            let _ = ctx.events_tx.send(CoreEvent::PeerUpdated(summary));
                        }
                    }

                    let now_ms = Utc::now().timestamp_millis();
                    let last_dial = dial_cooldowns.get(&peer_id).copied().unwrap_or(0);
                    if !already_connected && (now_ms - last_dial) > 2000 {
                        dial_cooldowns.insert(peer_id.clone(), now_ms);
                        connect_and_register_peer(ctx.clone(), peer_tcp_addr);
                    }
                    if dial_cooldowns.len() > 256 {
                        dial_cooldowns.retain(|_, ts| now_ms - *ts < 30_000);
                    }
                }
            }
        }
    })
}

fn spawn_maintenance_loop(ctx: Arc<NodeContext>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        let mut tick: u32 = 0;

        loop {
            interval.tick().await;
            tick = tick.wrapping_add(1);
            let now = Utc::now().timestamp_millis();

            // 1. Purge expired ephemeral messages & stale punches
            if let Ok(v) = ctx.vault.lock() {
                if let Ok(expired) = v.purge_expired_messages(now) {
                    for (chat_id, message_id) in expired {
                        let _ = ctx
                            .events_tx
                            .send(CoreEvent::MessageDeleted { chat_id, message_id });
                    }
                }
            }
            if let Ok(mut pending) = ctx.pending_punches.lock() {
                pending.retain(|_, p| now - p.created_at < 20_000);
            }
            if let Ok(mut nonces) = ctx.pending_echo_nonces.lock() {
                nonces.retain(|_, ts| now - *ts < 20_000);
            }

            // 2. Retry known offline peers & bootstrap seeds every 6s (or every 2s if completely disconnected)
            let has_active_peers = !ctx.active_peers.read().await.is_empty();
            if tick % 3 == 0 || !has_active_peers {
                let (seeds, onion_only) = ctx
                    .settings
                    .read()
                    .map(|s| (s.bootstrap_peers.clone(), s.onion_routing_enabled))
                    .unwrap_or_default();

                if !onion_only {
                    let connected: HashSet<String> =
                        ctx.active_peers.read().await.keys().cloned().collect();
                    let offline_candidates: Vec<String> = ctx
                        .vault
                        .lock()
                        .ok()
                        .and_then(|v| v.load_all_peers().ok())
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|p| {
                            if !connected.contains(&p.profile.peer_id) {
                                p.last_addr
                            } else {
                                None
                            }
                        })
                        .collect();

                    for addr_str in offline_candidates {
                        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                            connect_and_register_peer(ctx.clone(), addr);
                        }
                    }
                }
                trigger_mesh_bootstrap(&ctx, &seeds).await;
            }

            // 3. Retry unacknowledged outbox items via mesh relay (every 2s)
            {
                let unacked: Vec<SealedMeshPacket> = {
                    ctx.vault
                        .lock()
                        .ok()
                        .and_then(|v| v.load_outbox().ok())
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|item| now - item.created_at > 1_500)
                        .map(|item| item.sealed)
                        .collect()
                };
                if !unacked.is_empty() {
                    let relay_senders: Vec<mpsc::Sender<WireFrame>> = ctx
                        .active_peers
                        .read()
                        .await
                        .values()
                        .filter(|c| c.accepts_relay)
                        .map(|c| c.tx.clone())
                        .collect();
                    for tx in relay_senders {
                        for pkt in &unacked {
                            let _ = tx.send(WireFrame::SealedRelay(pkt.clone())).await;
                        }
                    }
                }
            }

            // 4. Re-announce SignedDhtRecord, flush DHT contacts to encrypted vault, send UDP keepalives
            if tick % 4 == 0 {
                announce_own_dht_record(&ctx).await;

                // Persist DHT routing table for zero-server blackout survival across restarts
                if let Ok(contacts) = ctx.routing_table.read().map(|t| t.all_contacts()) {
                    if let Ok(v) = ctx.vault.lock() {
                        let _ = v.save_dht_contacts(&contacts);
                    }
                }

                // UDP tunnel keepalives + expiry
                let tunnels: Vec<(String, ActiveUdpTunnel)> = ctx
                    .udp_tunnels
                    .read()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                for (pid, tun) in tunnels {
                    if now - tun.last_rx_ms > 12_000 {
                        cleanup_expired_udp_peer(&ctx, &pid).await;
                    } else if let Ok(bytes) = serde_json::to_vec(&UdpMeshPacket::TunnelKeepalive {
                        from_peer_id: ctx.keys.peer_id(),
                        ts: now,
                    }) {
                        let _ = ctx.udp_socket.send_to(&bytes, tun.remote_udp_addr).await;
                    }
                }

                // Send TCP Pings & emit updated NetworkTelemetry
                let senders: Vec<mpsc::Sender<WireFrame>> = ctx
                    .active_peers
                    .read()
                    .await
                    .values()
                    .map(|c| c.tx.clone())
                    .collect();
                for tx in senders {
                    let _ = tx.send(WireFrame::Ping(now)).await;
                }
                if let Ok(tel) = compute_telemetry(&ctx).await {
                    let _ = ctx.events_tx.send(CoreEvent::NetworkStatus(tel));
                }
            }
        }
    })
}

fn spawn_upnp_loop(ctx: Arc<NodeContext>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(900));
        loop {
            interval.tick().await;
            if let Some(mapped) = crate::upnp::discover_and_map_ports(ctx.tcp_port, ctx.udp_port).await {
                let candidate = SocketAddr::new(IpAddr::V4(mapped.external_ip), ctx.udp_port).to_string();
                let mut updated = false;
                if let Ok(mut w) = ctx.observed_external_addr.write() {
                    let should_set = w
                        .as_ref()
                        .and_then(|s| s.parse::<SocketAddr>().ok())
                        .map(|sock| is_private_or_local_ip(sock.ip()))
                        .unwrap_or(true);
                    if should_set {
                        *w = Some(candidate);
                        updated = true;
                    }
                }
                if updated {
                    announce_own_dht_record(&ctx).await;
                    if let Ok(tel) = compute_telemetry(&ctx).await {
                        let _ = ctx.events_tx.send(CoreEvent::NetworkStatus(tel));
                    }
                }
            }
        }
    })
}

fn connect_and_register_peer(ctx: Arc<NodeContext>, addr: SocketAddr) {
    tokio::spawn(async move {
        let Ok(my_prof) = ctx.own_profile.read().map(|p| p.clone()) else {
            return;
        };
        if let Ok(channel) = NoiseChannel::connect(
            addr,
            &ctx.keys.x25519_secret(),
            &ctx.keys.signing_key(),
            &my_prof,
        )
        .await
        {
            let _ = register_connected_channel(&ctx, channel, Some(addr.to_string())).await;
        }
    });
}

async fn register_connected_channel(
    ctx: &Arc<NodeContext>,
    channel: NoiseChannel,
    addr_hint: Option<String>,
) -> Result<PeerSummary> {
    let observed_sock = channel.remote_addr();
    let (mut reader, mut writer, peer_profile) = channel.split();
    let peer_id = peer_profile.peer_id.clone();
    let my_id = ctx.keys.peer_id();
    if peer_id == my_id {
        return Err(AegisError::Network("Self connection ignored".into()));
    }

    let x_bytes = decode_32_b58(&peer_profile.x25519_pub)?;
    let x_b58 = peer_profile.x25519_pub.clone();

    let route = match observed_sock {
        Some(sock) if !is_private_or_local_ip(sock.ip()) => ConnectionRoute::WanDirect,
        _ => ConnectionRoute::LanDirect,
    };
    let remote_addr_str = observed_sock
        .map(|s| s.to_string())
        .or_else(|| addr_hint.clone());

    let hidden_relay = peer_profile.is_hidden_relay();
    let mut summary = if hidden_relay {
        peer_profile.verify()?;
        let (safety_number, safety_emojis) = compute_safety_verification(&my_id, &peer_id)?;
        let fingerprint = format_fingerprint(&peer_id);
        let account_id = format_account_id(&peer_profile.handle, &peer_id);
        PeerSummary {
            profile: peer_profile,
            account_id,
            fingerprint,
            route,
            latency_ms: None,
            verified: false,
            safety_number,
            safety_emojis,
            last_addr: addr_hint,
            last_seen: Utc::now().timestamp_millis(),
            unread_count: 0,
            last_message_preview: None,
            last_message_at: None,
            pinned: false,
        }
    } else {
        upsert_peer_in_vault(&ctx.vault, &my_id, peer_profile, addr_hint)?
    };
    summary.route = route;

    // Seed Kademlia routing table with the connected peer
    if let Ok(mut table) = ctx.routing_table.write() {
        table.upsert(DhtNodeContact {
            peer_id: peer_id.clone(),
            x25519_pub: x_b58,
            tcp_addrs: remote_addr_str.clone().into_iter().collect(),
            udp_addr: None,
            relay_capable: true,
            last_seen: Utc::now().timestamp_millis(),
            failures: 0,
        });
    }

    let (tx, mut rx) = mpsc::channel::<WireFrame>(256);
    {
        let mut peers_map = ctx.active_peers.write().await;
        peers_map.insert(
            peer_id.clone(),
            ActivePeerConn {
                tx: tx.clone(),
                route,
                remote_addr: remote_addr_str,
                udp_port: observed_sock.map(|s| s.port()).unwrap_or(ctx.udp_port),
                accepts_relay: true,
                x25519_pub: x_bytes,
                is_udp: false,
            },
        );
    }
    if !hidden_relay {
        let _ = ctx.events_tx.send(CoreEvent::PeerUpdated(summary.clone()));
    }
    if let Ok(tel) = compute_telemetry(ctx).await {
        let _ = ctx.events_tx.send(CoreEvent::NetworkStatus(tel));
    }

    // Exchange Peer-Observed IP + DHT PEX + DHT Signed Record BEFORE flushing mailbox
    // so recipients hydrate sender DHT identity records cleanly before sealed messages arrive.
    let accepts_relay = ctx
        .settings
        .read()
        .map(|s| s.mesh_relay_enabled)
        .unwrap_or(true);
    if let Some(sock) = observed_sock {
        let _ = tx
            .send(WireFrame::PeerObservedAddr {
                observed_ip: sock.ip().to_string(),
                observed_tcp_port: sock.port(),
                peer_udp_port: ctx.udp_port,
                peer_accepts_relay: accepts_relay,
            })
            .await;
    }
    if ctx.settings.read().map(|s| s.dht_enabled).unwrap_or(true) {
        let _ = tx
            .send(WireFrame::DhtFindNode {
                req_id: Uuid::new_v4().to_string(),
                target_peer_id: my_id.clone(),
            })
            .await;
        announce_own_dht_record(ctx).await;

        let now = Utc::now().timestamp_millis();
        let cached_records: Vec<SignedDhtRecord> = ctx
            .dht_records
            .read()
            .ok()
            .map(|s| s.all_valid(now).into_iter().take(16).collect())
            .unwrap_or_default();
        for cached in cached_records {
            if cached.peer_id != peer_id {
                let _ = tx.send(WireFrame::DhtAnnounceRecord(cached)).await;
            }
        }
        let _ = tx.send(WireFrame::DhtFetchMailbox { peer_id: my_id }).await;
    }

    // Flush queued offline outbox & relay mailbox packets
    flush_outbox_and_mesh_for_peer(ctx, &peer_id, &tx).await;

    // Writer task
    let writer_handle = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if writer.send_frame(&frame).await.is_err() {
                break;
            }
        }
    });
    let writer_abort = writer_handle.abort_handle();

    // Reader task
    let ctx_r = ctx.clone();
    let peer_id_clone = peer_id.clone();
    let tx_clone = tx.clone();

    let reader_handle = tokio::spawn(async move {
        while let Ok(frame) = reader.recv_frame().await {
            handle_incoming_frame(&ctx_r, frame, &peer_id_clone, &tx_clone).await;
        }
        writer_handle.abort();

        let should_mark_offline = {
            let mut map = ctx_r.active_peers.write().await;
            if let Some(current) = map.get(&peer_id_clone) {
                if current.tx.same_channel(&tx_clone) {
                    map.remove(&peer_id_clone);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if should_mark_offline {
            if let Ok(mut w) = ctx_r.peer_latencies.write() {
                w.remove(&peer_id_clone);
            }
            if let Ok(v) = ctx_r.vault.lock() {
                if let Ok(peers) = v.load_all_peers() {
                    if let Some(mut p) = peers
                        .into_iter()
                        .find(|p| p.profile.peer_id == peer_id_clone && !p.profile.is_hidden_relay())
                    {
                        p.route = ConnectionRoute::Offline;
                        p.latency_ms = None;
                        p.last_seen = Utc::now().timestamp_millis();
                        let _ = v.save_peer(&p);
                        let _ = ctx_r.events_tx.send(CoreEvent::PeerUpdated(p));
                    }
                }
            }
            if let Ok(tel) = compute_telemetry(&ctx_r).await {
                let _ = ctx_r.events_tx.send(CoreEvent::NetworkStatus(tel));
            }
        }
    });

    if let Ok(mut list) = ctx.peer_handles.lock() {
        list.retain(|h| !h.is_finished());
        list.push(writer_abort);
        list.push(reader_handle.abort_handle());
    }

    Ok(summary)
}

async fn handle_incoming_frame(
    ctx: &Arc<NodeContext>,
    frame: WireFrame,
    peer_id: &str,
    peer_tx: &mpsc::Sender<WireFrame>,
) {
    let my_id = ctx.keys.peer_id();
    match frame {
        WireFrame::ProfileSync(card) => {
            if card.peer_id == peer_id && card.verify().is_ok() && !card.is_hidden_relay() {
                if let Ok(mut summary) = upsert_peer_in_vault(&ctx.vault, &my_id, card, None) {
                    summary.route = ctx
                        .active_peers
                        .read()
                        .await
                        .get(peer_id)
                        .map(|c| c.route)
                        .unwrap_or(ConnectionRoute::LanDirect);
                    let _ = ctx.events_tx.send(CoreEvent::PeerUpdated(summary));
                }
            }
        }
        WireFrame::DirectMessage {
            msg_id,
            reply_to_id,
            expires_in_secs,
            timestamp,
            envelope,
        } => {
            if process_decrypted_ratchet_message(
                ctx,
                peer_id,
                msg_id.clone(),
                reply_to_id,
                expires_in_secs,
                timestamp,
                &envelope,
            ) {
                send_delivery_receipt_auto(ctx, peer_id, &msg_id, DeliveryStatus::Delivered).await;
            }
        }
        WireFrame::DeliveryReceipt { msg_id, status } => {
            handle_delivery_receipt(ctx, peer_id, &msg_id, status);
        }
        WireFrame::Typing { is_typing } => {
            let _ = ctx.events_tx.send(CoreEvent::TypingStatus {
                peer_id: peer_id.to_string(),
                is_typing,
            });
        }
        WireFrame::Reaction {
            msg_id,
            emoji,
            sender_id,
        } => {
            if sender_id != peer_id {
                return;
            }
            let mut updated_msg = None;
            if let Ok(v) = ctx.vault.lock() {
                if let Ok(msgs) = v.load_messages(peer_id) {
                    if let Some(mut target) = msgs.into_iter().find(|m| m.id == msg_id) {
                        if let Some(pos) = target
                            .reactions
                            .iter()
                            .position(|r| r.emoji == emoji && r.sender_id == peer_id)
                        {
                            target.reactions.remove(pos);
                        } else {
                            target.reactions.push(MessageReaction {
                                emoji,
                                sender_id: peer_id.to_string(),
                            });
                        }
                        if v.save_message(&target).is_ok() {
                            updated_msg = Some(target);
                        }
                    }
                }
            }
            if let Some(m) = updated_msg {
                let _ = ctx.events_tx.send(CoreEvent::MessageUpdated(m));
            }
        }
        WireFrame::EditMessage {
            msg_id,
            new_content,
        } => {
            let mut updated_msg = None;
            if let Ok(v) = ctx.vault.lock() {
                if let Ok(msgs) = v.load_messages(peer_id) {
                    if let Some(mut target) = msgs
                        .into_iter()
                        .find(|m| m.id == msg_id && m.sender_id == peer_id)
                    {
                        target.content = new_content;
                        target.edited = true;
                        if v.save_message(&target).is_ok() {
                            updated_msg = Some(target);
                        }
                    }
                }
            }
            if let Some(m) = updated_msg {
                let _ = ctx.events_tx.send(CoreEvent::MessageUpdated(m));
            }
        }
        WireFrame::DeleteMessage { msg_id } => {
            let mut deleted = false;
            if let Ok(v) = ctx.vault.lock() {
                if let Ok(msgs) = v.load_messages(peer_id) {
                    if msgs.iter().any(|m| m.id == msg_id && m.sender_id == peer_id) {
                        deleted = v.delete_message(peer_id, &msg_id).unwrap_or(false);
                    }
                }
            }
            if deleted {
                let _ = ctx.events_tx.send(CoreEvent::MessageDeleted {
                    chat_id: peer_id.to_string(),
                    message_id: msg_id,
                });
            }
        }
        WireFrame::Ping(ts) => {
            let _ = peer_tx.send(WireFrame::Pong(ts)).await;
        }
        WireFrame::Pong(ts) => {
            let now = Utc::now().timestamp_millis();
            let rtt = (now - ts).clamp(1, 9999) as u32;
            if let Ok(mut map) = ctx.peer_latencies.write() {
                map.insert(peer_id.to_string(), rtt);
            }
            let cur_route = ctx
                .active_peers
                .read()
                .await
                .get(peer_id)
                .map(|c| c.route)
                .unwrap_or(ConnectionRoute::LanDirect);
            if let Ok(v) = ctx.vault.lock() {
                if let Ok(peers) = v.load_all_peers() {
                    if let Some(mut p) = peers
                        .into_iter()
                        .find(|p| p.profile.peer_id == peer_id && !p.profile.is_hidden_relay())
                    {
                        p.route = cur_route;
                        p.latency_ms = Some(rtt);
                        let _ = ctx.events_tx.send(CoreEvent::PeerUpdated(p));
                    }
                }
            }
        }
        WireFrame::PeerObservedAddr {
            observed_ip,
            observed_tcp_port: _,
            peer_udp_port,
            peer_accepts_relay,
        } => {
            let mut peer_remote_ip: Option<IpAddr> = None;
            {
                let mut map = ctx.active_peers.write().await;
                if let Some(conn) = map.get_mut(peer_id) {
                    conn.udp_port = peer_udp_port;
                    conn.accepts_relay = peer_accepts_relay;
                    if let Some(ref addr) = conn.remote_addr {
                        if let Ok(sock) = addr.parse::<SocketAddr>() {
                            peer_remote_ip = Some(sock.ip());
                        }
                    }
                }
            }
            if let Ok(ip) = observed_ip.parse::<IpAddr>() {
                if !is_private_or_local_ip(ip)
                    || ctx
                        .observed_external_addr
                        .read()
                        .ok()
                        .map(|o| o.is_none())
                        .unwrap_or(false)
                {
                    if let Ok(mut w) = ctx.observed_external_addr.write() {
                        *w = Some(SocketAddr::new(ip, ctx.udp_port).to_string());
                    }
                }
            }
            // Ask peer via UDP Echo to learn true external NAT-mapped UDP port (zero Google STUN!)
            if let Some(ip) = peer_remote_ip {
                let nonce: u64 = rand::random();
                if let Ok(mut m) = ctx.pending_echo_nonces.lock() {
                    m.insert(nonce, Utc::now().timestamp_millis());
                }
                let echo = UdpMeshPacket::EchoReq {
                    from_peer_id: my_id,
                    nonce,
                };
                if let Ok(bytes) = serde_json::to_vec(&echo) {
                    let _ = ctx
                        .udp_socket
                        .send_to(&bytes, SocketAddr::new(ip, peer_udp_port))
                        .await;
                }
            }
        }
        WireFrame::DhtFindNode {
            req_id,
            target_peer_id,
        } => {
            let contacts = ctx
                .routing_table
                .read()
                .map(|t| t.find_closest(&target_peer_id, 16))
                .unwrap_or_default();
            let _ = peer_tx
                .send(WireFrame::DhtFindNodeResp { req_id, contacts })
                .await;
        }
        WireFrame::DhtFindNodeResp { contacts, .. } => {
            if let Ok(mut table) = ctx.routing_table.write() {
                for c in contacts {
                    table.upsert(c);
                }
            }
        }
        WireFrame::DhtAnnounceRecord(rec) => {
            let now = Utc::now().timestamp_millis();
            let is_new = ctx
                .dht_records
                .write()
                .ok()
                .and_then(|mut s| s.upsert(rec.clone(), now).ok())
                .unwrap_or(false);
            if is_new {
                if let Ok(v) = ctx.vault.lock() {
                    let _ = v.save_dht_record(&rec);
                }
                if let Ok(mut table) = ctx.routing_table.write() {
                    table.upsert(DhtNodeContact {
                        peer_id: rec.peer_id.clone(),
                        x25519_pub: rec.x25519_pub.clone(),
                        tcp_addrs: rec.tcp_addrs.clone(),
                        udp_addr: rec.udp_punch_addr.clone(),
                        relay_capable: rec.accepts_relay,
                        last_seen: rec.published_at,
                        failures: 0,
                    });
                }
                // Resolve any awaiting DHT query waiting specifically for this peer_id
                if let Ok(mut pending) = ctx.pending_dht_queries.lock() {
                    let matching_keys: Vec<String> = pending
                        .iter()
                        .filter(|(_, (target_id, _))| target_id == &rec.peer_id)
                        .map(|(k, _)| k.clone())
                        .collect();
                    for k in matching_keys {
                        if let Some((_, tx)) = pending.remove(&k) {
                            let _ = tx.send(Some(rec.clone()));
                        }
                    }
                }
                // Bounded mesh gossip propagation (only triggers once per strictly newer seq)
                let neighbors: Vec<(String, mpsc::Sender<WireFrame>)> = ctx
                    .active_peers
                    .read()
                    .await
                    .iter()
                    .filter(|(id, _)| id.as_str() != peer_id && id.as_str() != rec.peer_id)
                    .map(|(id, c)| (id.clone(), c.tx.clone()))
                    .take(4)
                    .collect();
                for (_, tx) in neighbors {
                    let _ = tx.send(WireFrame::DhtAnnounceRecord(rec.clone())).await;
                }
            }
        }
        WireFrame::DhtWithdrawRecord {
            peer_id: departing_id,
            signature,
        } => {
            let verified = if let Ok(ed_bytes) = decode_32_b58(&departing_id) {
                if let Ok(verifying_key) = ed25519_dalek::VerifyingKey::from_bytes(&ed_bytes) {
                    if let Ok(sig_bytes) = bs58::decode(&signature).into_vec() {
                        if let Ok(sig) = ed25519_dalek::Signature::from_slice(&sig_bytes) {
                            verifying_key.verify_strict(b"CYMESH_DHT_WITHDRAW", &sig).is_ok()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if verified {
                if let Ok(mut store) = ctx.dht_records.write() {
                    store.remove(&departing_id);
                }
                if let Ok(mut table) = ctx.routing_table.write() {
                    table.remove(&departing_id);
                }
                let neighbors: Vec<mpsc::Sender<WireFrame>> = ctx
                    .active_peers
                    .read()
                    .await
                    .iter()
                    .filter(|(id, _)| id.as_str() != peer_id && id.as_str() != &departing_id)
                    .map(|(_, c)| c.tx.clone())
                    .take(4)
                    .collect();
                for tx in neighbors {
                    let _ = tx
                        .send(WireFrame::DhtWithdrawRecord {
                            peer_id: departing_id.clone(),
                            signature: signature.clone(),
                        })
                        .await;
                }
            }
        }
        WireFrame::DhtQueryRecord {
            req_id,
            target_peer_id,
        } => {
            let now = Utc::now().timestamp_millis();
            let record = ctx
                .dht_records
                .read()
                .ok()
                .and_then(|s| s.get(&target_peer_id, now));
            let closer_contacts = ctx
                .routing_table
                .read()
                .map(|t| t.find_closest(&target_peer_id, 12))
                .unwrap_or_default();
            let _ = peer_tx
                .send(WireFrame::DhtQueryRecordResp {
                    req_id,
                    record,
                    closer_contacts,
                })
                .await;
        }
        WireFrame::DhtQueryRecordResp {
            req_id,
            record,
            closer_contacts,
        } => {
            let now = Utc::now().timestamp_millis();
            if let Ok(mut table) = ctx.routing_table.write() {
                for c in closer_contacts {
                    table.upsert(c);
                }
            }
            if let Some(rec) = record {
                if let Ok(mut store) = ctx.dht_records.write() {
                    if store.upsert(rec.clone(), now).unwrap_or(false) {
                        if let Ok(v) = ctx.vault.lock() {
                            let _ = v.save_dht_record(&rec);
                        }
                    }
                }
                if let Ok(mut pending) = ctx.pending_dht_queries.lock() {
                    if let Some((_, tx)) = pending.remove(&req_id) {
                        let _ = tx.send(Some(rec));
                    }
                }
            }
        }
        WireFrame::DhtSearchAccount { req_id, query, ttl } => {
            let fresh_search = {
                let mut seen = ctx.seen_search_ids.lock().unwrap_or_else(|e| e.into_inner());
                if seen.len() > 2048 {
                    seen.clear();
                }
                seen.insert(req_id.clone())
            };
            if !fresh_search {
                return;
            }
            let now = Utc::now().timestamp_millis();
            let matches: Vec<SignedDhtRecord> = ctx
                .dht_records
                .read()
                .map(|s| {
                    s.search_accounts(&query, now)
                        .into_iter()
                        .map(|(rec, _)| rec)
                        .collect()
                })
                .unwrap_or_default();

            let neighbors: Vec<mpsc::Sender<WireFrame>> = if ttl > 1 {
                ctx.active_peers
                    .read()
                    .await
                    .iter()
                    .filter(|(id, _)| id.as_str() != peer_id)
                    .map(|(_, c)| c.tx.clone())
                    .take(6)
                    .collect()
            } else {
                Vec::new()
            };

            if !matches.is_empty() || neighbors.is_empty() {
                let _ = peer_tx
                    .send(WireFrame::DhtSearchAccountResp {
                        req_id: req_id.clone(),
                        records: matches,
                    })
                    .await;
            }

            if !neighbors.is_empty() {
                if let Ok(mut routes) = ctx.search_reply_routes.lock() {
                    if routes.len() > 2048 {
                        routes.clear();
                    }
                    routes.insert(req_id.clone(), peer_id.to_string());
                }
                for tx in neighbors {
                    let _ = tx
                        .send(WireFrame::DhtSearchAccount {
                            req_id: req_id.clone(),
                            query: query.clone(),
                            ttl: ttl - 1,
                        })
                        .await;
                }
            }
        }
        WireFrame::DhtSearchAccountResp { req_id, records } => {
            let now = Utc::now().timestamp_millis();
            let mut verified = Vec::new();
            for rec in records {
                if rec.verify(now).is_ok() {
                    if let Ok(mut store) = ctx.dht_records.write() {
                        if store.upsert(rec.clone(), now).unwrap_or(false) {
                            if let Ok(v) = ctx.vault.lock() {
                                let _ = v.save_dht_record(&rec);
                            }
                        }
                    }
                    verified.push(rec);
                }
            }
            let local_tx = ctx
                .pending_account_searches
                .lock()
                .ok()
                .and_then(|p| p.get(&req_id).cloned());
            if let Some(tx) = local_tx {
                let _ = tx.send(verified);
            } else if !verified.is_empty() {
                let upstream_peer = ctx
                    .search_reply_routes
                    .lock()
                    .ok()
                    .and_then(|r| r.get(&req_id).cloned());
                if let Some(upstream_id) = upstream_peer {
                    if let Some(conn) = ctx.active_peers.read().await.get(&upstream_id).cloned() {
                        let _ = conn
                            .tx
                            .send(WireFrame::DhtSearchAccountResp {
                                req_id,
                                records: verified,
                            })
                            .await;
                    }
                }
            }
        }
        WireFrame::DhtFetchMailbox {
            peer_id: requester_id,
        } => {
            if requester_id != peer_id {
                return;
            }
            let now = Utc::now().timestamp_millis();
            let my_tag = compute_route_tag(&requester_id, "global-mesh");
            let stored = ctx
                .vault
                .lock()
                .ok()
                .and_then(|v| v.load_mesh_relay_packets(now).ok())
                .unwrap_or_default();
            for pkt in stored {
                if pkt.route_tag == my_tag {
                    if peer_tx.send(WireFrame::SealedRelay(pkt.clone())).await.is_ok() {
                        if let Ok(v) = ctx.vault.lock() {
                            let _ = v.remove_mesh_relay_packet(&pkt.packet_id);
                        }
                    }
                }
            }
        }
        WireFrame::OnionRelay(onion) => {
            handle_incoming_onion(ctx, onion, peer_id).await;
        }
        WireFrame::PunchCoordinate {
            punch_id,
            initiator_peer_id,
            initiator_x25519_pub,
            initiator_udp_addr,
            target_peer_id,
        } => {
            if target_peer_id == my_id {
                handle_punch_coordinate_target(
                    ctx,
                    peer_tx,
                    punch_id,
                    initiator_peer_id,
                    initiator_x25519_pub,
                    initiator_udp_addr,
                )
                .await;
            } else {
                // Relay forward toward target peer
                if let Some(target_conn) = ctx.active_peers.read().await.get(&target_peer_id).cloned() {
                    if let Ok(mut routes) = ctx.punch_relay_routes.lock() {
                        routes.insert(punch_id.clone(), peer_id.to_string());
                    }
                    let _ = target_conn
                        .tx
                        .send(WireFrame::PunchCoordinate {
                            punch_id,
                            initiator_peer_id,
                            initiator_x25519_pub,
                            initiator_udp_addr,
                            target_peer_id,
                        })
                        .await;
                }
            }
        }
        WireFrame::PunchReady {
            punch_id,
            responder_peer_id,
            responder_x25519_pub,
            responder_udp_addr,
            initiator_peer_id,
        } => {
            if initiator_peer_id == my_id {
                handle_punch_ready_initiator(
                    ctx,
                    punch_id,
                    responder_peer_id,
                    responder_x25519_pub,
                    responder_udp_addr,
                )
                .await;
            } else {
                let return_hop = ctx
                    .punch_relay_routes
                    .lock()
                    .ok()
                    .and_then(|mut m| m.remove(&punch_id))
                    .unwrap_or_else(|| initiator_peer_id.clone());
                if let Some(init_conn) = ctx.active_peers.read().await.get(&return_hop).cloned() {
                    let _ = init_conn
                        .tx
                        .send(WireFrame::PunchReady {
                            punch_id,
                            responder_peer_id,
                            responder_x25519_pub,
                            responder_udp_addr,
                            initiator_peer_id,
                        })
                        .await;
                }
            }
        }
        WireFrame::SealedRelay(pkt) => {
            process_sealed_relay_packet(ctx, pkt, peer_id).await;
        }
    }
}

async fn handle_incoming_onion(
    ctx: &Arc<NodeContext>,
    onion: OnionPacket,
    from_peer_id: &str,
) {
    let already_seen = {
        let mut seen = ctx.seen_mesh_ids.lock().unwrap_or_else(|e| e.into_inner());
        if seen.len() > 4096 {
            seen.clear();
        }
        !seen.insert(onion.packet_id.clone())
    };
    if already_seen || onion.hop_ttl == 0 {
        return;
    }

    let Ok(layer) = peel_onion_layer(&onion, &ctx.keys.x25519_secret()) else {
        return;
    };

    match layer {
        OnionLayerPayload::Forward {
            next_peer_id,
            mut next_packet,
        } => {
            if next_packet.hop_ttl == 0 {
                return;
            }
            next_packet.hop_ttl -= 1;
            ctx.mesh_relayed_count.fetch_add(1, Ordering::Relaxed);
            if let Some(next_conn) = ctx.active_peers.read().await.get(&next_peer_id).cloned() {
                let _ = next_conn.tx.send(WireFrame::OnionRelay(next_packet)).await;
            } else if let Some(contact) = ctx
                .routing_table
                .read()
                .ok()
                .and_then(|t| t.get(&next_peer_id))
            {
                for addr_str in contact.tcp_addrs {
                    if let Ok(sock) = addr_str.parse::<SocketAddr>() {
                        spawn_connect_and_forward_onion(
                            ctx.clone(),
                            sock,
                            next_peer_id.clone(),
                            next_packet.clone(),
                        );
                        break;
                    }
                }
            }
        }
        OnionLayerPayload::DeliverSealed {
            recipient_peer_id,
            sealed_packet,
        } => {
            if recipient_peer_id == ctx.keys.peer_id() {
                process_sealed_relay_packet(ctx, sealed_packet, from_peer_id).await;
                return;
            }
            ctx.mesh_relayed_count.fetch_add(1, Ordering::Relaxed);

            if let Some(target_conn) = ctx
                .active_peers
                .read()
                .await
                .get(&recipient_peer_id)
                .cloned()
            {
                let _ = target_conn
                    .tx
                    .send(WireFrame::SealedRelay(sealed_packet))
                    .await;
            } else {
                // Recipient is offline or attached elsewhere: store in encrypted DHT Mailbox
                if let Ok(v) = ctx.vault.lock() {
                    let _ = v.save_mesh_relay_packet(&sealed_packet);
                }
                let mesh_on = ctx
                    .settings
                    .read()
                    .map(|s| s.mesh_relay_enabled)
                    .unwrap_or(true);
                if mesh_on {
                    let peers: Vec<mpsc::Sender<WireFrame>> = ctx
                        .active_peers
                        .read()
                        .await
                        .iter()
                        .filter(|(id, c)| id.as_str() != from_peer_id && c.accepts_relay)
                        .map(|(_, c)| c.tx.clone())
                        .collect();
                    for tx in peers {
                        let _ = tx.send(WireFrame::SealedRelay(sealed_packet.clone())).await;
                    }
                }
            }
        }
    }
}

fn spawn_connect_and_forward_onion(
    ctx: Arc<NodeContext>,
    sock: SocketAddr,
    target_peer_id: String,
    pkt: OnionPacket,
) {
    tokio::spawn(async move {
        let Ok(my_prof) = ctx.own_profile.read().map(|p| p.clone()) else {
            return;
        };
        if let Ok(ch) = NoiseChannel::connect(
            sock,
            &ctx.keys.x25519_secret(),
            &ctx.keys.signing_key(),
            &my_prof,
        )
        .await
        {
            let _ = register_connected_channel(&ctx, ch, Some(sock.to_string())).await;
            if let Some(c) = ctx.active_peers.read().await.get(&target_peer_id).cloned() {
                let _ = c.tx.send(WireFrame::OnionRelay(pkt)).await;
            }
        }
    });
}

async fn process_sealed_relay_packet(
    ctx: &Arc<NodeContext>,
    mut pkt: SealedMeshPacket,
    from_peer_id: &str,
) {
    let my_id = ctx.keys.peer_id();
    let already_seen = {
        let mut seen = ctx.seen_mesh_ids.lock().unwrap_or_else(|e| e.into_inner());
        if seen.len() > 4096 {
            seen.clear();
        }
        !seen.insert(pkt.packet_id.clone())
    };
    if already_seen {
        return;
    }

    let my_tag = compute_route_tag(&my_id, "global-mesh");
    if pkt.route_tag == my_tag {
        if let Ok(inner) = unseal_packet(&pkt, &my_id, &ctx.keys.x25519_secret()) {
            if let Some(ref prof) = inner.sender_profile {
                if prof.peer_id == inner.sender_peer_id {
                    let _ = upsert_peer_in_vault(&ctx.vault, &my_id, prof.clone(), None);
                }
            }
            match serde_json::from_slice::<WireFrame>(&inner.payload_json) {
                Ok(WireFrame::DirectMessage {
                    msg_id,
                    reply_to_id,
                    expires_in_secs,
                    timestamp,
                    envelope,
                }) => {
                    if process_decrypted_ratchet_message(
                        ctx,
                        &inner.sender_peer_id,
                        msg_id.clone(),
                        reply_to_id,
                        expires_in_secs,
                        timestamp,
                        &envelope,
                    ) {
                        send_delivery_receipt_auto(
                            ctx,
                            &inner.sender_peer_id,
                            &msg_id,
                            DeliveryStatus::Delivered,
                        )
                        .await;
                    }
                }
                Ok(WireFrame::DeliveryReceipt { msg_id, status }) => {
                    handle_delivery_receipt(ctx, &inner.sender_peer_id, &msg_id, status);
                }
                Ok(WireFrame::Reaction {
                    msg_id,
                    emoji,
                    sender_id,
                }) => {
                    if sender_id == inner.sender_peer_id {
                        let mut updated_msg = None;
                        if let Ok(v) = ctx.vault.lock() {
                            if let Ok(msgs) = v.load_messages(&inner.sender_peer_id) {
                                if let Some(mut target) = msgs.into_iter().find(|m| m.id == msg_id) {
                                    if let Some(pos) = target
                                        .reactions
                                        .iter()
                                        .position(|r| r.emoji == emoji && r.sender_id == inner.sender_peer_id)
                                    {
                                        target.reactions.remove(pos);
                                    } else {
                                        target.reactions.push(MessageReaction {
                                            emoji,
                                            sender_id: inner.sender_peer_id.clone(),
                                        });
                                    }
                                    if v.save_message(&target).is_ok() {
                                        updated_msg = Some(target);
                                    }
                                }
                            }
                        }
                        if let Some(m) = updated_msg {
                            let _ = ctx.events_tx.send(CoreEvent::MessageUpdated(m));
                        }
                    }
                }
                Ok(WireFrame::EditMessage {
                    msg_id,
                    new_content,
                }) => {
                    let mut updated_msg = None;
                    if let Ok(v) = ctx.vault.lock() {
                        if let Ok(msgs) = v.load_messages(&inner.sender_peer_id) {
                            if let Some(mut target) = msgs
                                .into_iter()
                                .find(|m| m.id == msg_id && m.sender_id == inner.sender_peer_id)
                            {
                                target.content = new_content;
                                target.edited = true;
                                if v.save_message(&target).is_ok() {
                                    updated_msg = Some(target);
                                }
                            }
                        }
                    }
                    if let Some(m) = updated_msg {
                        let _ = ctx.events_tx.send(CoreEvent::MessageUpdated(m));
                    }
                }
                Ok(WireFrame::DeleteMessage { msg_id }) => {
                    let mut deleted = false;
                    if let Ok(v) = ctx.vault.lock() {
                        if v.delete_message(&inner.sender_peer_id, &msg_id).is_ok() {
                            deleted = true;
                        }
                    }
                    if deleted {
                        let _ = ctx.events_tx.send(CoreEvent::MessageDeleted {
                            chat_id: inner.sender_peer_id.clone(),
                            message_id: msg_id,
                        });
                    }
                }
                _ => {}
            }
        }
        return;
    }

    let mesh_enabled = ctx
        .settings
        .read()
        .map(|s| s.mesh_relay_enabled)
        .unwrap_or(true);
    if mesh_enabled && pkt.hop_ttl > 1 && pkt.expires_at > Utc::now().timestamp_millis() {
        pkt.hop_ttl -= 1;
        if let Ok(v) = ctx.vault.lock() {
            let _ = v.save_mesh_relay_packet(&pkt);
        }
        ctx.mesh_relayed_count.fetch_add(1, Ordering::Relaxed);

        let targets: Vec<mpsc::Sender<WireFrame>> = ctx
            .active_peers
            .read()
            .await
            .iter()
            .filter(|(id, c)| id.as_str() != from_peer_id && c.accepts_relay)
            .map(|(_, c)| c.tx.clone())
            .collect();
        for tx in targets {
            let _ = tx.send(WireFrame::SealedRelay(pkt.clone())).await;
        }
    }
}

async fn handle_punch_coordinate_target(
    ctx: &Arc<NodeContext>,
    relay_tx: &mpsc::Sender<WireFrame>,
    punch_id: String,
    initiator_peer_id: String,
    initiator_x25519_pub: String,
    initiator_udp_addr: String,
) {
    let punch_on = ctx
        .settings
        .read()
        .map(|s| s.hole_punch_enabled)
        .unwrap_or(true);
    if !punch_on {
        return;
    }
    let Ok(init_udp) = initiator_udp_addr.parse::<SocketAddr>() else {
        return;
    };
    let Ok(init_eph_x) = decode_32_b58(&initiator_x25519_pub) else {
        return;
    };
    let Some(init_static_x) = resolve_peer_x25519(ctx, &initiator_peer_id) else {
        return;
    };

    let my_id = ctx.keys.peer_id();
    let resp_eph_secret = X25519Static::random_from_rng(rand::rngs::OsRng);
    let resp_eph_pub_b58 =
        bs58::encode(X25519Public::from(&resp_eph_secret).as_bytes()).into_string();

    let cipher = UdpTunnelCipher::derive(
        &my_id,
        &initiator_peer_id,
        &ctx.keys.x25519_secret(),
        &X25519Public::from(init_static_x),
        &resp_eph_secret,
        &X25519Public::from(init_eph_x),
        &punch_id,
    );

    if let Ok(mut pending) = ctx.pending_punches.lock() {
        pending.insert(
            punch_id.clone(),
            PendingPunchState {
                target_peer_id: initiator_peer_id.clone(),
                target_static_x: init_static_x,
                my_eph_secret: resp_eph_secret,
                my_eph_pub_b58: resp_eph_pub_b58.clone(),
                target_udp_addr: Some(init_udp),
                created_at: Utc::now().timestamp_millis(),
            },
        );
    }

    let my_udp_addr = ctx
        .observed_external_addr
        .read()
        .ok()
        .and_then(|o| o.clone())
        .unwrap_or_else(|| {
            let ip = enumerate_local_ipv4s()
                .into_iter()
                .next()
                .unwrap_or(Ipv4Addr::LOCALHOST);
            format!("{ip}:{}", ctx.udp_port)
        });

    let _ = relay_tx
        .send(WireFrame::PunchReady {
            punch_id: punch_id.clone(),
            responder_peer_id: my_id.clone(),
            responder_x25519_pub: resp_eph_pub_b58.clone(),
            responder_udp_addr: my_udp_addr,
            initiator_peer_id: initiator_peer_id.clone(),
        })
        .await;

    let _ = cipher; // Derived cleanly when PunchHello / PunchAck completes
    let sig = sign_punch_handshake(
        &ctx.keys,
        "HELLO",
        &punch_id,
        &initiator_peer_id,
        &resp_eph_pub_b58,
    );
    let hello = UdpMeshPacket::PunchHello {
        from_peer_id: my_id,
        to_peer_id: initiator_peer_id,
        punch_id,
        eph_x_pub: resp_eph_pub_b58,
        sig,
    };
    if let Ok(bytes) = serde_json::to_vec(&hello) {
        let udp_sock = ctx.udp_socket.clone();
        tokio::spawn(async move {
            for _ in 0..8 {
                let _ = udp_sock.send_to(&bytes, init_udp).await;
                tokio::time::sleep(Duration::from_millis(85)).await;
            }
        });
    }
}

async fn handle_punch_ready_initiator(
    ctx: &Arc<NodeContext>,
    punch_id: String,
    responder_peer_id: String,
    responder_x25519_pub: String,
    responder_udp_addr: String,
) {
    let Ok(resp_udp) = responder_udp_addr.parse::<SocketAddr>() else {
        return;
    };
    let Ok(resp_eph_x) = decode_32_b58(&responder_x25519_pub) else {
        return;
    };

    let mut derived: Option<(UdpTunnelCipher, String)> = None;
    if let Ok(mut pending) = ctx.pending_punches.lock() {
        if let Some(p_state) = pending.get_mut(&punch_id) {
            if p_state.target_peer_id == responder_peer_id {
                let cipher = UdpTunnelCipher::derive(
                    &ctx.keys.peer_id(),
                    &responder_peer_id,
                    &ctx.keys.x25519_secret(),
                    &X25519Public::from(p_state.target_static_x),
                    &p_state.my_eph_secret,
                    &X25519Public::from(resp_eph_x),
                    &punch_id,
                );
                p_state.target_udp_addr = Some(resp_udp);
                derived = Some((cipher, p_state.my_eph_pub_b58.clone()));
            }
        }
    }

    if let Some((_cipher, my_eph_pub)) = derived {
        let sig = sign_punch_handshake(
            &ctx.keys,
            "HELLO",
            &punch_id,
            &responder_peer_id,
            &my_eph_pub,
        );
        let hello = UdpMeshPacket::PunchHello {
            from_peer_id: ctx.keys.peer_id(),
            to_peer_id: responder_peer_id,
            punch_id,
            eph_x_pub: my_eph_pub,
            sig,
        };
        if let Ok(bytes) = serde_json::to_vec(&hello) {
            let udp_sock = ctx.udp_socket.clone();
            tokio::spawn(async move {
                for _ in 0..8 {
                    let _ = udp_sock.send_to(&bytes, resp_udp).await;
                    tokio::time::sleep(Duration::from_millis(85)).await;
                }
            });
        }
    }
}

fn resolve_peer_x25519(ctx: &Arc<NodeContext>, peer_id: &str) -> Option<[u8; 32]> {
    if let Ok(v) = ctx.vault.lock() {
        if let Ok(peers) = v.load_all_peers() {
            if let Some(p) = peers.into_iter().find(|p| p.profile.peer_id == peer_id) {
                if let Ok(x) = decode_32_b58(&p.profile.x25519_pub) {
                    return Some(x);
                }
            }
        }
    }
    let now = Utc::now().timestamp_millis();
    if let Ok(store) = ctx.dht_records.read() {
        if let Some(rec) = store.get(peer_id, now) {
            if let Ok(x) = decode_32_b58(&rec.x25519_pub) {
                return Some(x);
            }
        }
    }
    if let Ok(rt) = ctx.routing_table.read() {
        if let Some(c) = rt.get(peer_id) {
            if let Ok(x) = decode_32_b58(&c.x25519_pub) {
                return Some(x);
            }
        }
    }
    None
}

fn process_decrypted_ratchet_message(
    ctx: &Arc<NodeContext>,
    sender_id: &str,
    msg_id: String,
    reply_to_id: Option<String>,
    expires_in_secs: Option<u64>,
    timestamp: i64,
    envelope: &crate::crypto::RatchetEnvelope,
) -> bool {
    let my_id = ctx.keys.peer_id();
    let Ok(v) = ctx.vault.lock() else {
        return false;
    };
    let Ok(peers) = v.load_all_peers() else {
        return false;
    };

    // If sender is known in vault, use it; otherwise hydrate from verified DHT record if present
    let peer_idx = peers.iter().position(|p| p.profile.peer_id == sender_id);
    if peer_idx.is_none() {
        drop(v);
        let rec_opt = ctx
            .dht_records
            .read()
            .ok()
            .and_then(|s| s.get(sender_id, Utc::now().timestamp_millis()));
        let Some(rec) = rec_opt else {
            return false;
        };
        if upsert_peer_from_dht_record(ctx, &rec).is_err() {
            return false;
        }
    } else {
        drop(v);
    }

    let Ok(v) = ctx.vault.lock() else {
        return false;
    };
    let Ok(mut peers) = v.load_all_peers() else {
        return false;
    };
    let Some(peer) = peers.iter_mut().find(|p| p.profile.peer_id == sender_id) else {
        return false;
    };
    let Ok(their_x_bytes) = decode_32_b58(&peer.profile.x25519_pub) else {
        return false;
    };
    let their_x_pub = X25519Public::from(their_x_bytes);

    let mut ratchet = match v.load_ratchet_state(sender_id) {
        Ok(Some(r)) => r,
        Ok(None) => DoubleRatchetState::initialize(
            &ctx.keys.x25519_secret(),
            &ctx.keys.x25519_public(),
            &their_x_pub,
        ),
        Err(_) => return false,
    };

    let (plain_bytes, ratchet_to_save) = match ratchet.decrypt(envelope) {
        Ok(bytes) => (bytes, ratchet),
        Err(err) => {
            if envelope.prev_chain_len == 0 && envelope.msg_num == 0 {
                let mut fresh = DoubleRatchetState::initialize(
                    &ctx.keys.x25519_secret(),
                    &ctx.keys.x25519_public(),
                    &their_x_pub,
                );
                match fresh.decrypt(envelope) {
                    Ok(bytes) => (bytes, fresh),
                    Err(_) => {
                        tracing::warn!("Double Ratchet decryption failed for message from {sender_id}: {err}");
                        return false;
                    }
                }
            } else {
                tracing::warn!("Double Ratchet decryption failed for message from {sender_id}: {err}");
                return false;
            }
        }
    };
    if v.save_ratchet_state(sender_id, &ratchet_to_save).is_err() {
        return false;
    }

    let content = String::from_utf8_lossy(&plain_bytes).to_string();
    let now = Utc::now().timestamp_millis();
    let expires_at = expires_in_secs.map(|s| now + (s as i64) * 1000);

    let chat_msg = ChatMessage {
        id: msg_id,
        chat_id: sender_id.to_string(),
        sender_id: sender_id.to_string(),
        recipient_id: my_id,
        content: content.clone(),
        reply_to_id,
        reactions: Vec::new(),
        timestamp,
        expires_at,
        status: DeliveryStatus::Delivered,
        edited: false,
        ratchet_counter: envelope.msg_num as u64,
    };

    if v.save_message(&chat_msg).is_err() {
        return false;
    }
    let direct_route = ctx
        .active_peers
        .try_read()
        .ok()
        .and_then(|m| m.get(sender_id).map(|c| c.route));
    if let Some(r) = direct_route {
        peer.route = r;
    } else if peer.route == ConnectionRoute::Offline {
        peer.route = ConnectionRoute::MeshRelay;
    }
    peer.unread_count = peer.unread_count.saturating_add(1);
    peer.last_message_preview = Some(preview_text(&content));
    peer.last_message_at = Some(timestamp);
    peer.last_seen = now;
    let _ = v.save_peer(peer);
    let peer_clone = peer.clone();
    drop(v);

    let _ = ctx.events_tx.send(CoreEvent::MessageReceived(chat_msg));
    let _ = ctx.events_tx.send(CoreEvent::PeerUpdated(peer_clone));
    true
}

async fn flush_outbox_and_mesh_for_peer(
    ctx: &Arc<NodeContext>,
    peer_id: &str,
    tx: &mpsc::Sender<WireFrame>,
) {
    let now = Utc::now().timestamp_millis();
    let (direct_items, relay_items) = {
        let Ok(v) = ctx.vault.lock() else {
            return;
        };
        let outbox = v.load_outbox().unwrap_or_default();
        let mesh_pkts = v.load_mesh_relay_packets(now).unwrap_or_default();
        (outbox, mesh_pkts)
    };

    let target_tag = compute_route_tag(peer_id, "global-mesh");
    for item in direct_items {
        if item.recipient_id == peer_id {
            if tx.send(WireFrame::SealedRelay(item.sealed)).await.is_ok() {
                if let Ok(v) = ctx.vault.lock() {
                    let _ = v.remove_outbox(&item.msg_id);
                    if let Ok(msgs) = v.load_messages(peer_id) {
                        if let Some(mut msg) = msgs.into_iter().find(|m| m.id == item.msg_id) {
                            msg.status = DeliveryStatus::SentDirect;
                            let _ = v.save_message(&msg);
                        }
                    }
                }
            }
        }
    }

    let mesh_enabled = ctx
        .settings
        .read()
        .map(|s| s.mesh_relay_enabled)
        .unwrap_or(true);
    for pkt in relay_items {
        if pkt.route_tag == target_tag {
            if tx.send(WireFrame::SealedRelay(pkt.clone())).await.is_ok() {
                if let Ok(v) = ctx.vault.lock() {
                    let _ = v.remove_mesh_relay_packet(&pkt.packet_id);
                }
            }
        } else if mesh_enabled {
            let _ = tx.send(WireFrame::SealedRelay(pkt)).await;
        }
    }
}

fn upsert_peer_in_vault(
    vault: &Arc<Mutex<UnlockedVault>>,
    my_id: &str,
    profile: SignedProfileCard,
    addr_hint: Option<String>,
) -> Result<PeerSummary> {
    profile.verify()?;
    let now = Utc::now().timestamp_millis();
    let v = vault
        .lock()
        .map_err(|_| AegisError::Storage("Vault lock".into()))?;
    let existing = v
        .load_all_peers()?
        .into_iter()
        .find(|p| p.profile.peer_id == profile.peer_id);

    let (safety_number, safety_emojis) = compute_safety_verification(my_id, &profile.peer_id)?;
    let fingerprint = format_fingerprint(&profile.peer_id);

    let summary = if let Some(mut prev) = existing {
        if profile.updated_at >= prev.profile.updated_at {
            prev.profile = profile;
        }
        prev.account_id = format_account_id(&prev.profile.handle, &prev.profile.peer_id);
        if let Some(a) = addr_hint {
            prev.last_addr = Some(a);
        }
        prev.last_seen = now;
        prev
    } else {
        let account_id = format_account_id(&profile.handle, &profile.peer_id);
        PeerSummary {
            profile,
            account_id,
            fingerprint,
            route: ConnectionRoute::Offline,
            latency_ms: None,
            verified: false,
            safety_number,
            safety_emojis,
            last_addr: addr_hint,
            last_seen: now,
            unread_count: 0,
            last_message_preview: None,
            last_message_at: None,
            pinned: false,
        }
    };

    v.save_peer(&summary)?;
    Ok(summary)
}

async fn compute_telemetry(ctx: &Arc<NodeContext>) -> Result<NetworkTelemetry> {
    let local_ips: Vec<String> = enumerate_local_ipv4s()
        .into_iter()
        .map(|ip| ip.to_string())
        .collect();
    let mut addrs: Vec<String> = if local_ips.is_empty() {
        vec![format!("127.0.0.1:{}", ctx.tcp_port)]
    } else {
        local_ips
            .iter()
            .map(|ip| format!("{ip}:{}", ctx.tcp_port))
            .collect()
    };

    let observed_external_addr = ctx
        .observed_external_addr
        .read()
        .ok()
        .and_then(|o| o.clone());
    if let Some(ref ext) = observed_external_addr {
        if let Ok(sock) = ext.parse::<SocketAddr>() {
            let ext_tcp = SocketAddr::new(sock.ip(), ctx.tcp_port).to_string();
            if !addrs.contains(&ext_tcp) {
                addrs.insert(0, ext_tcp);
            }
        }
    }

    let prof = ctx
        .own_profile
        .read()
        .map_err(|_| AegisError::Storage("Profile lock".into()))?
        .clone();
    let account_id = format_account_id(&prof.handle, &prof.peer_id);
    let share_uri = encode_share_uri(&prof, &addrs)?;
    let stealth_mode = ctx
        .settings
        .read()
        .map(|s| s.stealth_mode)
        .unwrap_or(false);
    let peers_snapshot = ctx.active_peers.read().await;
    let active_direct_peers = peers_snapshot.len();
    let active_mesh_relays = peers_snapshot.values().filter(|c| c.accepts_relay).count();
    drop(peers_snapshot);

    let queued_offline_messages = ctx
        .vault
        .lock()
        .map(|v| v.load_outbox().map(|o| o.len()).unwrap_or(0))
        .unwrap_or(0);
    let dht_known_nodes = ctx.routing_table.read().map(|t| t.len()).unwrap_or(0);
    let dht_stored_records = ctx.dht_records.read().map(|s| s.len()).unwrap_or(0);

    Ok(NetworkTelemetry {
        node_listening_port: ctx.tcp_port,
        local_ips,
        share_uri,
        active_direct_peers,
        queued_offline_messages,
        mesh_relayed_packets: ctx.mesh_relayed_count.load(Ordering::Relaxed),
        stealth_mode,
        observed_external_addr,
        account_id,
        dht_known_nodes,
        dht_stored_records,
        active_mesh_relays,
        nat_punch_successes: ctx.nat_punch_successes.load(Ordering::Relaxed),
        onion_routed_packets: ctx.onion_routed_count.load(Ordering::Relaxed),
    })
}

fn preview_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() > 48 {
        let mut s: String = trimmed.chars().take(48).collect();
        s.push('…');
        s
    } else {
        trimmed.to_string()
    }
}

fn normalize_handle(handle: &str) -> String {
    let h = handle.trim();
    if h.is_empty() {
        "@anon".to_string()
    } else if h.starts_with('@') {
        h.to_string()
    } else {
        format!("@{h}")
    }
}

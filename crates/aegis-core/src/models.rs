use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AegisError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    Online,
    Stealth,
    Busy,
    Away,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionRoute {
    LanDirect,
    WanDirect,
    MeshRelay,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    QueuedOffline,
    RelayedMesh,
    SentDirect,
    Delivered,
    Read,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultUnlockMode {
    Primary,
    Decoy,
    Wiped,
}

/// Cryptographically signed decentralized profile card.
/// No central server stores or issues profiles; peers verify signatures directly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedProfileCard {
    pub peer_id: String,
    pub x25519_pub: String,
    pub display_name: String,
    pub handle: String,
    pub bio: String,
    pub role_badge: String,
    pub avatar_color: String,
    pub avatar_data: Option<String>,
    pub status: PresenceStatus,
    pub updated_at: i64,
    pub signature: String,
}

impl SignedProfileCard {
    /// Canonical byte representation covered by the Ed25519 signature.
    fn canonical_payload(&self) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(b"AegisMesh-Profile-v1:");
        for field in [
            self.peer_id.as_str(),
            self.x25519_pub.as_str(),
            self.display_name.as_str(),
            self.handle.as_str(),
            self.bio.as_str(),
            self.role_badge.as_str(),
            self.avatar_color.as_str(),
            self.avatar_data.as_deref().unwrap_or(""),
        ] {
            let bytes = field.as_bytes();
            hasher.update((bytes.len() as u32).to_le_bytes());
            hasher.update(bytes);
        }
        let status_byte = match self.status {
            PresenceStatus::Online => 1u8,
            PresenceStatus::Stealth => 2u8,
            PresenceStatus::Busy => 3u8,
            PresenceStatus::Away => 4u8,
        };
        hasher.update([status_byte]);
        hasher.update(self.updated_at.to_le_bytes());
        hasher.finalize().to_vec()
    }

    pub fn sign(&mut self, signing_key: &SigningKey) {
        let payload = self.canonical_payload();
        let sig: Signature = signing_key.sign(&payload);
        self.signature = bs58::encode(sig.to_bytes()).into_string();
    }

    pub fn verify(&self) -> Result<()> {
        if self.bio.chars().count() > 500 {
            return Err(AegisError::InvalidPayload("Bio exceeds 500 characters limit".into()));
        }

        if let Some(ref data) = self.avatar_data {
            if data.len() > 65536 {
                return Err(AegisError::InvalidPayload("Avatar data exceeds 64KB limit".into()));
            }
            if !data.starts_with("data:image/webp;base64,")
                && !data.starts_with("data:image/jpeg;base64,")
                && !data.starts_with("data:image/jpg;base64,")
                && !data.starts_with("data:image/png;base64,")
            {
                return Err(AegisError::InvalidPayload(
                    "Avatar data must be a safe base64 raster image data URI".into(),
                ));
            }
        }

        let raw_pk = bs58::decode(&self.peer_id)
            .into_vec()
            .map_err(|e| AegisError::InvalidKey(e.to_string()))?;
        let pk_arr: [u8; 32] = raw_pk
            .try_into()
            .map_err(|_| AegisError::InvalidKey("Ed25519 key must be 32 bytes".into()))?;
        let verifying_key = VerifyingKey::from_bytes(&pk_arr)
            .map_err(|e| AegisError::InvalidKey(e.to_string()))?;

        let raw_x = bs58::decode(&self.x25519_pub)
            .into_vec()
            .map_err(|e| AegisError::InvalidKey(e.to_string()))?;
        if raw_x.len() != 32 {
            return Err(AegisError::InvalidKey("X25519 key must be 32 bytes".into()));
        }

        let raw_sig = bs58::decode(&self.signature)
            .into_vec()
            .map_err(|_| AegisError::InvalidSignature(self.peer_id.clone()))?;
        let sig_arr: [u8; 64] = raw_sig
            .try_into()
            .map_err(|_| AegisError::InvalidSignature(self.peer_id.clone()))?;
        let sig = Signature::from_bytes(&sig_arr);

        let payload = self.canonical_payload();
        verifying_key
            .verify(&payload, &sig)
            .map_err(|_| AegisError::InvalidSignature(self.peer_id.clone()))?;
        Ok(())
    }

    /// Returns true for headless blind relay infrastructure nodes (`role_badge == "Relay Node"`).
    /// These participate in DHT/routing/relay without cluttering user chat lists.
    pub fn is_hidden_relay(&self) -> bool {
        self.role_badge.eq_ignore_ascii_case("Relay Node")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReaction {
    pub emoji: String,
    pub sender_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub chat_id: String,
    pub sender_id: String,
    pub recipient_id: String,
    pub content: String,
    pub reply_to_id: Option<String>,
    pub reactions: Vec<MessageReaction>,
    pub timestamp: i64,
    pub expires_at: Option<i64>,
    pub status: DeliveryStatus,
    pub edited: bool,
    pub ratchet_counter: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerSummary {
    pub profile: SignedProfileCard,
    #[serde(default)]
    pub account_id: String,
    pub fingerprint: String,
    pub route: ConnectionRoute,
    pub latency_ms: Option<u32>,
    pub verified: bool,
    pub safety_number: String,
    pub safety_emojis: Vec<String>,
    pub last_addr: Option<String>,
    pub last_seen: i64,
    pub unread_count: u32,
    pub last_message_preview: Option<String>,
    pub last_message_at: Option<i64>,
    pub pinned: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpnAdapterInfo {
    pub vpn_detected: bool,
    pub adapter_name: Option<String>,
    pub proxy_detected: bool,
    pub proxy_details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub language: String,
    pub stealth_mode: bool,
    pub mesh_relay_enabled: bool,
    pub listen_port: u16,
    pub auto_lock_minutes: u32,
    pub default_ephemeral_secs: Option<u64>,
    #[serde(default = "default_true")]
    pub dht_enabled: bool,
    #[serde(default = "default_true")]
    pub hole_punch_enabled: bool,
    #[serde(default)]
    pub onion_routing_enabled: bool,
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,
    #[serde(default)]
    pub enforce_iran_ip: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: "fa".to_string(),
            stealth_mode: false,
            mesh_relay_enabled: true,
            listen_port: 47833,
            auto_lock_minutes: 15,
            default_ephemeral_secs: None,
            dht_enabled: true,
            hole_punch_enabled: true,
            onion_routing_enabled: false,
            bootstrap_peers: Vec::new(),
            enforce_iran_ip: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayHint {
    pub peer_id: String,
    pub x25519_pub: String,
    pub addrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhtNodeContact {
    pub peer_id: String,
    pub x25519_pub: String,
    pub tcp_addrs: Vec<String>,
    pub udp_addr: Option<String>,
    pub relay_capable: bool,
    pub last_seen: i64,
    #[serde(default)]
    pub failures: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTelemetry {
    pub node_listening_port: u16,
    pub local_ips: Vec<String>,
    pub share_uri: String,
    pub active_direct_peers: usize,
    pub queued_offline_messages: usize,
    pub mesh_relayed_packets: u64,
    pub stealth_mode: bool,
    #[serde(default)]
    pub observed_external_addr: Option<String>,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub dht_known_nodes: usize,
    #[serde(default)]
    pub dht_stored_records: usize,
    #[serde(default)]
    pub active_mesh_relays: usize,
    #[serde(default)]
    pub nat_punch_successes: u64,
    #[serde(default)]
    pub onion_routed_packets: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityCreatedBundle {
    pub profile: SignedProfileCard,
    #[serde(default)]
    pub account_id: String,
    pub recovery_mnemonic: String,
    pub fingerprint: String,
    pub share_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSearchResult {
    pub peer_id: String,
    pub account_id: String,
    pub profile: SignedProfileCard,
    pub direct_reachable: bool,
    pub relay_hops_available: usize,
    pub already_paired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolunteerRelayResult {
    pub addr: String,
    pub peer_id: String,
    pub latency_ms: u32,
    pub propagated_peers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerHealthResult {
    pub target: String,
    pub reachable: bool,
    pub latency_ms: Option<u32>,
    pub noise_handshake_ok: bool,
    pub peer_id: Option<String>,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub observed_ip: Option<String>,
    pub message: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum CoreEvent {
    PeerUpdated(PeerSummary),
    ChatDeleted { peer_id: String },
    MessageReceived(ChatMessage),
    MessageUpdated(ChatMessage),
    MessageDeleted { chat_id: String, message_id: String },
    TypingStatus { peer_id: String, is_typing: bool },
    NetworkStatus(NetworkTelemetry),
    VaultLocked,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityKeys;

    #[test]
    fn test_profile_card_security_validation() {
        let (keys, _) = IdentityKeys::generate().unwrap();

        // 1. Valid profile with safe WebP avatar and bio
        let valid_avatar = "data:image/webp;base64,UklGRiQAAABXRUJQVlA4IBgAAAAwAQCdASoBAAEAD8D+JaQAA3AA/ua1AAA=";
        let card = keys.build_signed_profile(
            "Alice",
            "@alice",
            "Secure mesh node operator",
            "Node",
            "#00F0FF",
            Some(valid_avatar.to_string()),
            PresenceStatus::Online,
        );
        assert!(card.verify().is_ok());

        // 2. Reject oversized bio (> 500 chars)
        let huge_bio = "A".repeat(501);
        let mut bad_bio_card = keys.build_signed_profile(
            "Alice",
            "@alice",
            huge_bio,
            "Node",
            "#00F0FF",
            None,
            PresenceStatus::Online,
        );
        bad_bio_card.sign(&keys.signing_key());
        assert!(bad_bio_card.verify().is_err());

        // 3. Reject oversized avatar (> 64KB)
        let huge_avatar = format!("data:image/webp;base64,{}", "A".repeat(66000));
        let mut bad_avatar_card = keys.build_signed_profile(
            "Alice",
            "@alice",
            "Short bio",
            "Node",
            "#00F0FF",
            Some(huge_avatar),
            PresenceStatus::Online,
        );
        bad_avatar_card.sign(&keys.signing_key());
        assert!(bad_avatar_card.verify().is_err());

        // 4. Reject dangerous schemes like javascript: or http:
        let xss_avatar = "javascript:alert(1)";
        let mut xss_card = keys.build_signed_profile(
            "Alice",
            "@alice",
            "Short bio",
            "Node",
            "#00F0FF",
            Some(xss_avatar.to_string()),
            PresenceStatus::Online,
        );
        xss_card.sign(&keys.signing_key());
        assert!(xss_card.verify().is_err());
    }
}

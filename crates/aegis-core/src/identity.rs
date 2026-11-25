use bip39::{Language, Mnemonic};
use chrono::Utc;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Static};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{AegisError, Result};
use crate::models::{PresenceStatus, SignedProfileCard};

const SAFETY_EMOJIS: [&str; 32] = [
    "🛡️", "🔐", "⚡", "🛰️", "🧬", "🗝️", "💎", "🧭",
    "🔥", "❄️", "🌊", "🌌", "🦊", "🦅", "🦉", "🐉",
    "🔮", "📡", "🔋", "⚙️", "🧪", "🧿", "💠", "🪐",
    "☄️", "🌿", "🏔️", "⚓", "🎯", "🎭", "🧩", "🚀",
];

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct IdentityKeys {
    pub master_seed: [u8; 32],
    signing_bytes: [u8; 32],
    x25519_bytes: [u8; 32],
}

impl IdentityKeys {
    /// Generates a brand new offline cryptographic identity with a 12-word recovery phrase.
    pub fn generate() -> Result<(Self, String)> {
        let mut entropy = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut entropy);
        let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)
            .map_err(|e| AegisError::InvalidMnemonic(e.to_string()))?;
        entropy.zeroize();
        let phrase = mnemonic.to_string();
        let keys = Self::from_mnemonic(&phrase)?;
        Ok((keys, phrase))
    }

    /// Deterministically derives Ed25519 + X25519 identity keys from a BIP39 recovery phrase.
    pub fn from_mnemonic(phrase: &str) -> Result<Self> {
        let mnemonic = Mnemonic::parse_in_normalized(Language::English, phrase.trim())
            .map_err(|e| AegisError::InvalidMnemonic(e.to_string()))?;
        let seed_bytes = mnemonic.to_seed("");
        let master_seed = blake3::derive_key("AegisMesh-Identity-Master-v1", &seed_bytes);
        Ok(Self::from_master_seed(master_seed))
    }

    pub fn from_master_seed(master_seed: [u8; 32]) -> Self {
        let signing_bytes = blake3::derive_key("AegisMesh-Ed25519-v1", &master_seed);
        let x25519_bytes = blake3::derive_key("AegisMesh-X25519-v1", &master_seed);
        Self {
            master_seed,
            signing_bytes,
            x25519_bytes,
        }
    }

    pub fn signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(&self.signing_bytes)
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key().verifying_key()
    }

    pub fn x25519_secret(&self) -> X25519Static {
        X25519Static::from(self.x25519_bytes)
    }

    pub fn x25519_public(&self) -> X25519Public {
        X25519Public::from(&self.x25519_secret())
    }

    pub fn peer_id(&self) -> String {
        bs58::encode(self.verifying_key().as_bytes()).into_string()
    }

    pub fn x25519_pub_b58(&self) -> String {
        bs58::encode(self.x25519_public().as_bytes()).into_string()
    }

    pub fn fingerprint(&self) -> String {
        format_fingerprint(&self.peer_id())
    }

    pub fn build_signed_profile(
        &self,
        display_name: impl Into<String>,
        handle: impl Into<String>,
        bio: impl Into<String>,
        role_badge: impl Into<String>,
        avatar_color: impl Into<String>,
        avatar_data: Option<String>,
        status: PresenceStatus,
    ) -> SignedProfileCard {
        let mut card = SignedProfileCard {
            peer_id: self.peer_id(),
            x25519_pub: self.x25519_pub_b58(),
            display_name: display_name.into(),
            handle: handle.into(),
            bio: bio.into(),
            role_badge: role_badge.into(),
            avatar_color: avatar_color.into(),
            avatar_data,
            status,
            updated_at: Utc::now().timestamp_millis(),
            signature: String::new(),
        };
        card.sign(&self.signing_key());
        card
    }
}

pub fn format_fingerprint(peer_id: &str) -> String {
    let digest = blake3::hash(peer_id.as_bytes());
    let hex_str = hex::encode(&digest.as_bytes()[..6]).to_uppercase();
    format!("{}-{}-{}", &hex_str[0..4], &hex_str[4..8], &hex_str[8..12])
}

/// Computes symmetric Signal-style Safety Number (30 digits) and 6 verification emojis
/// from both peers' Ed25519 public keys, identical regardless of who computes it.
pub fn compute_safety_verification(peer_a_b58: &str, peer_b_b58: &str) -> Result<(String, Vec<String>)> {
    let a = decode_32_b58(peer_a_b58)?;
    let b = decode_32_b58(peer_b_b58)?;

    let (first, second) = if a <= b { (a, b) } else { (b, a) };

    let mut hasher = Sha256::new();
    hasher.update(b"AegisMesh-Safety-v1:");
    hasher.update(first);
    hasher.update(second);
    let digest = hasher.finalize();

    // Derive 30 decimal digits in 6 groups of 5 digits
    let mut groups = Vec::with_capacity(6);
    for i in 0..6 {
        let idx = i * 4;
        let val = u32::from_be_bytes([digest[idx], digest[idx + 1], digest[idx + 2], digest[idx + 3]]) % 100_000;
        groups.push(format!("{val:05}"));
    }
    let safety_number = groups.join(" ");

    let mut emojis = Vec::with_capacity(6);
    for i in 0..6 {
        let emoji_idx = (digest[24 + i] as usize) % SAFETY_EMOJIS.len();
        emojis.push(SAFETY_EMOJIS[emoji_idx].to_string());
    }

    Ok((safety_number, emojis))
}

pub fn format_account_tag(peer_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"Cymesh-AccountTag-v1:");
    hasher.update(peer_id.as_bytes());
    let digest = hasher.finalize();
    format!("{:02X}{:02X}", digest[0], digest[1])
}

pub fn format_account_id(handle: &str, peer_id: &str) -> String {
    let clean = handle.trim().trim_start_matches('@');
    let base = if clean.is_empty() { "node" } else { clean };
    format!("@{}#{}", base, format_account_tag(peer_id))
}

pub fn decode_32_b58(val: &str) -> Result<[u8; 32]> {
    let raw = bs58::decode(val.trim())
        .into_vec()
        .map_err(|e| AegisError::InvalidKey(e.to_string()))?;
    raw.try_into()
        .map_err(|_| AegisError::InvalidKey("Expected 32-byte Base58 key".into()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerSharePayload {
    pub v: u8,
    pub profile: SignedProfileCard,
    pub addrs: Vec<String>,
}

pub fn encode_share_uri(profile: &SignedProfileCard, addrs: &[String]) -> Result<String> {
    let payload = PeerSharePayload {
        v: 1,
        profile: profile.clone(),
        addrs: addrs.to_vec(),
    };
    let json = serde_json::to_vec(&payload)?;
    let b58 = bs58::encode(json).into_string();
    Ok(format!("cymesh://id/{b58}"))
}

pub fn decode_share_uri(uri: &str) -> Result<PeerSharePayload> {
    let trimmed = uri.trim();
    let encoded = trimmed
        .strip_prefix("cymesh://id/")
        .or_else(|| trimmed.strip_prefix("cymesh://peer/"))
        .or_else(|| trimmed.strip_prefix("aegis://peer/"))
        .ok_or_else(|| AegisError::InvalidKey("Invalid Cymesh ID URI prefix".into()))?;
    let json = bs58::decode(encoded)
        .into_vec()
        .map_err(|e| AegisError::InvalidKey(e.to_string()))?;
    let payload: PeerSharePayload = serde_json::from_slice(&json)?;
    payload.profile.verify()?;
    Ok(payload)
}

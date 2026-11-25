use std::collections::HashMap;

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey as X25519Public, StaticSecret as X25519Static};
use zeroize::Zeroize;

use crate::crypto::SealedMeshPacket;
use crate::error::{AegisError, Result};
use crate::identity::{decode_32_b58, IdentityKeys};
use crate::models::{DhtNodeContact, RelayHint, SignedProfileCard};

const K_BUCKET_SIZE: usize = 20;
const MAX_DHT_CONTACTS: usize = 512;
const MAX_DHT_RECORDS: usize = 2048;
const MAX_ADDRS_PER_CONTACT: usize = 8;
const ONION_AAD: &[u8] = b"AegisMesh-Onion-AAD-v1";
const UDP_TUNNEL_AAD: &[u8] = b"AegisMesh-UdpTunnel-AAD-v1";

pub fn node_id_from_peer_id(peer_id: &str) -> [u8; 32] {
    *blake3::hash(peer_id.as_bytes()).as_bytes()
}

pub fn xor_distance(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = a[i] ^ b[i];
    }
    out
}

pub fn bucket_index(self_id: &[u8; 32], other_id: &[u8; 32]) -> usize {
    let dist = xor_distance(self_id, other_id);
    for (byte_idx, &byte) in dist.iter().enumerate() {
        if byte != 0 {
            let leading = byte.leading_zeros() as usize;
            return 255 - (byte_idx * 8 + leading);
        }
    }
    0
}

#[derive(Debug, Clone)]
pub struct DhtRoutingTable {
    self_peer_id: String,
    self_id: [u8; 32],
    buckets: Vec<Vec<DhtNodeContact>>,
}

impl DhtRoutingTable {
    pub fn new(self_peer_id: String) -> Self {
        let self_id = node_id_from_peer_id(&self_peer_id);
        Self {
            self_peer_id,
            self_id,
            buckets: vec![Vec::new(); 256],
        }
    }

    pub fn upsert(&mut self, mut contact: DhtNodeContact) {
        if contact.peer_id == self.self_peer_id || contact.peer_id.is_empty() {
            return;
        }
        if decode_32_b58(&contact.peer_id).is_err() || decode_32_b58(&contact.x25519_pub).is_err() {
            return;
        }
        contact.tcp_addrs.retain(|a| !a.trim().is_empty());
        dedup_keep_order(&mut contact.tcp_addrs, MAX_ADDRS_PER_CONTACT);

        let target_id = node_id_from_peer_id(&contact.peer_id);
        let b_idx = bucket_index(&self.self_id, &target_id);
        let bucket = &mut self.buckets[b_idx];

        if let Some(existing) = bucket.iter_mut().find(|c| c.peer_id == contact.peer_id) {
            for addr in contact.tcp_addrs {
                if !existing.tcp_addrs.contains(&addr) {
                    existing.tcp_addrs.push(addr);
                }
            }
            dedup_keep_order(&mut existing.tcp_addrs, MAX_ADDRS_PER_CONTACT);
            if contact.udp_addr.is_some() {
                existing.udp_addr = contact.udp_addr;
            }
            existing.relay_capable = contact.relay_capable;
            existing.last_seen = existing.last_seen.max(contact.last_seen);
            existing.failures = 0;
            return;
        }

        if bucket.len() < K_BUCKET_SIZE {
            bucket.push(contact);
        } else if let Some((oldest_idx, _)) = bucket
            .iter()
            .enumerate()
            .min_by_key(|(_, c)| (-(c.failures as i64), c.last_seen))
        {
            bucket[oldest_idx] = contact;
        }

        if self.len() > MAX_DHT_CONTACTS {
            self.prune_oldest();
        }
    }

    pub fn mark_failed(&mut self, peer_id: &str) {
        let target_id = node_id_from_peer_id(peer_id);
        let b_idx = bucket_index(&self.self_id, &target_id);
        let bucket = &mut self.buckets[b_idx];
        if let Some(pos) = bucket.iter().position(|c| c.peer_id == peer_id) {
            bucket[pos].failures = bucket[pos].failures.saturating_add(1);
            if bucket[pos].failures >= 4 {
                bucket.remove(pos);
            }
        }
    }

    pub fn get(&self, peer_id: &str) -> Option<DhtNodeContact> {
        let target_id = node_id_from_peer_id(peer_id);
        let b_idx = bucket_index(&self.self_id, &target_id);
        self.buckets[b_idx]
            .iter()
            .find(|c| c.peer_id == peer_id)
            .cloned()
    }

    pub fn remove(&mut self, peer_id: &str) {
        let target_id = node_id_from_peer_id(peer_id);
        let b_idx = bucket_index(&self.self_id, &target_id);
        self.buckets[b_idx].retain(|c| c.peer_id != peer_id);
    }

    pub fn find_closest(&self, target_peer_id: &str, limit: usize) -> Vec<DhtNodeContact> {
        let target_id = node_id_from_peer_id(target_peer_id);
        let mut all: Vec<DhtNodeContact> = self.buckets.iter().flatten().cloned().collect();
        all.sort_by_key(|c| xor_distance(&node_id_from_peer_id(&c.peer_id), &target_id));
        all.truncate(limit);
        all
    }

    pub fn all_contacts(&self) -> Vec<DhtNodeContact> {
        self.buckets.iter().flatten().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn prune_oldest(&mut self) {
        let mut oldest: Option<(usize, usize, i64)> = None;
        for (bi, bucket) in self.buckets.iter().enumerate() {
            for (ci, c) in bucket.iter().enumerate() {
                match oldest {
                    None => oldest = Some((bi, ci, c.last_seen)),
                    Some((_, _, ts)) if c.last_seen < ts => oldest = Some((bi, ci, c.last_seen)),
                    _ => {}
                }
            }
        }
        if let Some((bi, ci, _)) = oldest {
            self.buckets[bi].remove(ci);
        }
    }
}

fn dedup_keep_order(items: &mut Vec<String>, max_len: usize) {
    let mut unique = Vec::with_capacity(items.len().min(max_len));
    for item in items.drain(..) {
        if !unique.contains(&item) {
            unique.push(item);
            if unique.len() >= max_len {
                break;
            }
        }
    }
    *items = unique;
}

/// Cryptographically signed peer advertisement published into the serverless Kademlia DHT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedDhtRecord {
    pub peer_id: String,
    pub x25519_pub: String,
    pub tcp_addrs: Vec<String>,
    pub udp_punch_addr: Option<String>,
    pub relay_hints: Vec<RelayHint>,
    pub accepts_relay: bool,
    pub profile: Option<SignedProfileCard>,
    pub seq: u64,
    pub published_at: i64,
    pub expires_at: i64,
    pub signature: String,
}

impl SignedDhtRecord {
    fn canonical_payload(&self) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(b"AegisMesh-DHT-Record-v2:");
        for field in [
            self.peer_id.as_str(),
            self.x25519_pub.as_str(),
            self.udp_punch_addr.as_deref().unwrap_or(""),
        ] {
            let b = field.as_bytes();
            hasher.update((b.len() as u32).to_le_bytes());
            hasher.update(b);
        }
        hasher.update((self.tcp_addrs.len() as u32).to_le_bytes());
        for addr in &self.tcp_addrs {
            let b = addr.as_bytes();
            hasher.update((b.len() as u32).to_le_bytes());
            hasher.update(b);
        }
        hasher.update((self.relay_hints.len() as u32).to_le_bytes());
        for hint in &self.relay_hints {
            for s in [hint.peer_id.as_str(), hint.x25519_pub.as_str()] {
                let b = s.as_bytes();
                hasher.update((b.len() as u32).to_le_bytes());
                hasher.update(b);
            }
            hasher.update((hint.addrs.len() as u32).to_le_bytes());
            for a in &hint.addrs {
                let b = a.as_bytes();
                hasher.update((b.len() as u32).to_le_bytes());
                hasher.update(b);
            }
        }
        hasher.update([u8::from(self.accepts_relay)]);
        hasher.update(self.seq.to_le_bytes());
        hasher.update(self.published_at.to_le_bytes());
        hasher.update(self.expires_at.to_le_bytes());
        if let Some(prof) = &self.profile {
            hasher.update([1u8]);
            let sig_b = prof.signature.as_bytes();
            hasher.update((sig_b.len() as u32).to_le_bytes());
            hasher.update(sig_b);
        } else {
            hasher.update([0u8]);
        }
        hasher.finalize().to_vec()
    }

    pub fn sign(&mut self, signing_key: &SigningKey) {
        let payload = self.canonical_payload();
        let sig: Signature = signing_key.sign(&payload);
        self.signature = bs58::encode(sig.to_bytes()).into_string();
    }

    pub fn verify(&self, now_ms: i64) -> Result<()> {
        if self.expires_at <= now_ms {
            return Err(AegisError::Crypto("Expired DHT record".into()));
        }
        let pk_arr = decode_32_b58(&self.peer_id)?;
        let _x_arr = decode_32_b58(&self.x25519_pub)?;
        let vk = VerifyingKey::from_bytes(&pk_arr)
            .map_err(|e| AegisError::InvalidKey(e.to_string()))?;

        let raw_sig = bs58::decode(&self.signature)
            .into_vec()
            .map_err(|_| AegisError::InvalidSignature(self.peer_id.clone()))?;
        let sig_arr: [u8; 64] = raw_sig
            .try_into()
            .map_err(|_| AegisError::InvalidSignature(self.peer_id.clone()))?;
        let sig = Signature::from_bytes(&sig_arr);
        let payload = self.canonical_payload();
        vk.verify(&payload, &sig)
            .map_err(|_| AegisError::InvalidSignature(self.peer_id.clone()))?;

        if let Some(prof) = &self.profile {
            if prof.peer_id != self.peer_id {
                return Err(AegisError::InvalidSignature("Profile PeerId mismatch".into()));
            }
            prof.verify()?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct DhtRecordStore {
    records: HashMap<String, SignedDhtRecord>,
}

impl DhtRecordStore {
    pub fn upsert(&mut self, record: SignedDhtRecord, now_ms: i64) -> Result<bool> {
        record.verify(now_ms)?;
        if let Some(existing) = self.records.get(&record.peer_id) {
            if (record.seq, record.published_at) <= (existing.seq, existing.published_at) {
                return Ok(false);
            }
        }
        self.records.insert(record.peer_id.clone(), record);
        if self.records.len() > MAX_DHT_RECORDS {
            self.purge_expired_or_oldest(now_ms);
        }
        Ok(true)
    }

    pub fn get(&self, peer_id: &str, now_ms: i64) -> Option<SignedDhtRecord> {
        self.records
            .get(peer_id)
            .filter(|r| r.expires_at > now_ms)
            .cloned()
    }

    pub fn remove(&mut self, peer_id: &str) -> Option<SignedDhtRecord> {
        self.records.remove(peer_id)
    }

    pub fn all_valid(&self, now_ms: i64) -> Vec<SignedDhtRecord> {
        self.records
            .values()
            .filter(|r| r.expires_at > now_ms)
            .cloned()
            .collect()
    }

    /// Searches verified DHT records by @handle, @handle#TAG, display name, or Peer ID.
    /// Stealth accounts without a published profile are matched only by exact Peer ID.
    pub fn search_accounts(&self, query: &str, now_ms: i64) -> Vec<(SignedDhtRecord, String)> {
        let q = query.trim();
        let q_lower = q.to_lowercase();
        let q_no_at = q_lower.trim_start_matches('@');

        let mut scored: Vec<(u8, i64, SignedDhtRecord, String)> = Vec::new();
        for rec in self.records.values() {
            if rec.expires_at <= now_ms {
                continue;
            }
            if q == rec.peer_id {
                let handle = rec
                    .profile
                    .as_ref()
                    .map(|p| p.handle.as_str())
                    .unwrap_or("@node");
                let acct = crate::identity::format_account_id(handle, &rec.peer_id);
                scored.push((0, -rec.published_at, rec.clone(), acct));
                continue;
            }
            let Some(ref prof) = rec.profile else {
                continue;
            };
            let acct = crate::identity::format_account_id(&prof.handle, &rec.peer_id);
            if q.is_empty() {
                scored.push((4, -rec.published_at, rec.clone(), acct));
                continue;
            }

            let acct_lower = acct.to_lowercase();
            let acct_no_at = acct_lower.trim_start_matches('@');
            let handle_lower = prof.handle.trim().to_lowercase();
            let handle_no_at = handle_lower.trim_start_matches('@');
            let name_lower = prof.display_name.trim().to_lowercase();

            let rank = if acct_lower == q_lower || acct_no_at == q_no_at {
                Some(0)
            } else if !q_no_at.is_empty() && (handle_lower == q_lower || handle_no_at == q_no_at) {
                Some(1)
            } else if !q_no_at.is_empty()
                && (handle_no_at.starts_with(q_no_at) || acct_no_at.starts_with(q_no_at))
            {
                Some(2)
            } else if q_no_at.chars().count() >= 2
                && (handle_no_at.contains(q_no_at) || name_lower.contains(&q_lower))
            {
                Some(3)
            } else {
                None
            };

            if let Some(r) = rank {
                scored.push((r, -rec.published_at, rec.clone(), acct));
            }
        }

        scored.sort_by_key(|(rank, neg_ts, _, _)| (*rank, *neg_ts));
        scored
            .into_iter()
            .take(24)
            .map(|(_, _, rec, acct)| (rec, acct))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    fn purge_expired_or_oldest(&mut self, now_ms: i64) {
        self.records.retain(|_, r| r.expires_at > now_ms);
        while self.records.len() > MAX_DHT_RECORDS {
            if let Some(oldest_key) = self
                .records
                .iter()
                .min_by_key(|(_, r)| r.published_at)
                .map(|(k, _)| k.clone())
            {
                self.records.remove(&oldest_key);
            } else {
                break;
            }
        }
    }
}

/// Multi-hop blinded onion relay envelope. Each relay decrypts only its own hop layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnionPacket {
    pub packet_id: String,
    pub eph_pub: String,
    pub nonce: String,
    pub ciphertext: Vec<u8>,
    pub hop_ttl: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "val")]
pub enum OnionLayerPayload {
    Forward {
        next_peer_id: String,
        next_packet: OnionPacket,
    },
    DeliverSealed {
        recipient_peer_id: String,
        sealed_packet: SealedMeshPacket,
    },
}

fn derive_onion_hop_key(shared: &[u8; 32]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, shared);
    let mut key = [0u8; 32];
    hk.expand(b"AegisMesh-OnionHop-v1", &mut key)
        .expect("32-byte HKDF key is valid");
    key
}

fn encrypt_onion_layer(
    hop_x_pub: &[u8; 32],
    packet_id: String,
    payload: &OnionLayerPayload,
    hop_ttl: u8,
) -> Result<OnionPacket> {
    let eph = EphemeralSecret::random_from_rng(OsRng);
    let eph_pub = X25519Public::from(&eph);
    let shared = eph.diffie_hellman(&X25519Public::from(*hop_x_pub));
    let mut key = derive_onion_hop_key(shared.as_bytes());
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    key.zeroize();

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let plain = serde_json::to_vec(payload)?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: &plain,
                aad: ONION_AAD,
            },
        )
        .map_err(|_| AegisError::Crypto("Onion layer encryption failed".into()))?;

    Ok(OnionPacket {
        packet_id,
        eph_pub: bs58::encode(eph_pub.as_bytes()).into_string(),
        nonce: bs58::encode(nonce_bytes).into_string(),
        ciphertext,
        hop_ttl,
    })
}

/// Builds a multi-hop blinded onion packet wrapping an end-to-end `SealedMeshPacket`.
/// The first tuple in `hops` is the entry relay; the last tuple is the exit relay
/// responsible for forwarding `DeliverSealed` to `recipient_peer_id`.
pub fn build_onion_packet(
    hops: &[(String, [u8; 32])],
    recipient_peer_id: &str,
    sealed_packet: SealedMeshPacket,
) -> Result<OnionPacket> {
    if hops.is_empty() {
        return Err(AegisError::Network("Onion path requires at least 1 relay".into()));
    }

    let exit_idx = hops.len() - 1;
    let mut current_pkt = encrypt_onion_layer(
        &hops[exit_idx].1,
        format!("{}-hop{exit_idx}", sealed_packet.packet_id),
        &OnionLayerPayload::DeliverSealed {
            recipient_peer_id: recipient_peer_id.to_string(),
            sealed_packet: sealed_packet.clone(),
        },
        (hops.len() as u8).saturating_add(2),
    )?;

    for idx in (0..exit_idx).rev() {
        let next_peer_id = hops[idx + 1].0.clone();
        current_pkt = encrypt_onion_layer(
            &hops[idx].1,
            format!("{}-hop{idx}", sealed_packet.packet_id),
            &OnionLayerPayload::Forward {
                next_peer_id,
                next_packet: current_pkt,
            },
            (hops.len() as u8).saturating_add(2),
        )?;
    }

    Ok(current_pkt)
}

pub fn peel_onion_layer(
    packet: &OnionPacket,
    my_x_secret: &X25519Static,
) -> Result<OnionLayerPayload> {
    let eph_arr = decode_32_b58(&packet.eph_pub)?;
    let shared = my_x_secret.diffie_hellman(&X25519Public::from(eph_arr));
    let mut key = derive_onion_hop_key(shared.as_bytes());
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    key.zeroize();

    let raw_nonce = bs58::decode(&packet.nonce)
        .into_vec()
        .map_err(|_| AegisError::Crypto("Invalid onion nonce".into()))?;
    let nonce_arr: [u8; 12] = raw_nonce
        .try_into()
        .map_err(|_| AegisError::Crypto("Onion nonce must be 12 bytes".into()))?;

    let plain = cipher
        .decrypt(
            Nonce::from_slice(&nonce_arr),
            Payload {
                msg: &packet.ciphertext,
                aad: ONION_AAD,
            },
        )
        .map_err(|_| AegisError::Crypto("Onion layer decryption failed".into()))?;

    Ok(serde_json::from_slice(&plain)?)
}

/// UDP protocol for serverless Peer-Echo (STUN replacement), UDP Hole Punching,
/// and authenticated ChaCha20-Poly1305 peer tunnels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "u", content = "d")]
pub enum UdpMeshPacket {
    EchoReq {
        from_peer_id: String,
        nonce: u64,
    },
    EchoResp {
        from_peer_id: String,
        nonce: u64,
        observed_addr: String,
    },
    PunchHello {
        from_peer_id: String,
        to_peer_id: String,
        punch_id: String,
        eph_x_pub: String,
        sig: String,
    },
    PunchAck {
        from_peer_id: String,
        to_peer_id: String,
        punch_id: String,
        eph_x_pub: String,
        sig: String,
    },
    TunnelData {
        from_peer_id: String,
        nonce: [u8; 12],
        ciphertext: Vec<u8>,
    },
    TunnelKeepalive {
        from_peer_id: String,
        ts: i64,
    },
}

fn punch_auth_payload(
    kind: &str,
    punch_id: &str,
    from_peer_id: &str,
    to_peer_id: &str,
    eph_x_pub: &str,
) -> Vec<u8> {
    format!("AegisPunch-v1:{kind}:{punch_id}:{from_peer_id}:{to_peer_id}:{eph_x_pub}").into_bytes()
}

pub fn sign_punch_handshake(
    keys: &IdentityKeys,
    kind: &str,
    punch_id: &str,
    to_peer_id: &str,
    eph_x_pub: &str,
) -> String {
    let payload = punch_auth_payload(kind, punch_id, &keys.peer_id(), to_peer_id, eph_x_pub);
    let sig = keys.signing_key().sign(&payload);
    bs58::encode(sig.to_bytes()).into_string()
}

pub fn verify_punch_handshake(
    kind: &str,
    punch_id: &str,
    from_peer_id: &str,
    to_peer_id: &str,
    eph_x_pub: &str,
    sig_b58: &str,
) -> Result<()> {
    let pk_arr = decode_32_b58(from_peer_id)?;
    let _eph_arr = decode_32_b58(eph_x_pub)?;
    let vk =
        VerifyingKey::from_bytes(&pk_arr).map_err(|e| AegisError::InvalidKey(e.to_string()))?;
    let raw_sig = bs58::decode(sig_b58)
        .into_vec()
        .map_err(|_| AegisError::InvalidSignature(from_peer_id.to_string()))?;
    let sig_arr: [u8; 64] = raw_sig
        .try_into()
        .map_err(|_| AegisError::InvalidSignature(from_peer_id.to_string()))?;
    let sig = Signature::from_bytes(&sig_arr);
    let payload = punch_auth_payload(kind, punch_id, from_peer_id, to_peer_id, eph_x_pub);
    vk.verify(&payload, &sig)
        .map_err(|_| AegisError::InvalidSignature(from_peer_id.to_string()))
}

#[derive(Clone)]
pub struct UdpTunnelCipher {
    send_key: [u8; 32],
    recv_key: [u8; 32],
}

impl UdpTunnelCipher {
    /// Derives forward-secret directional tunnel keys from static + ephemeral X25519 agreements.
    pub fn derive(
        my_peer_id: &str,
        their_peer_id: &str,
        my_static_x: &X25519Static,
        their_static_x_pub: &X25519Public,
        my_eph_x: &X25519Static,
        their_eph_x_pub: &X25519Public,
        punch_id: &str,
    ) -> Self {
        let mut s_static = my_static_x.diffie_hellman(their_static_x_pub).to_bytes();
        let mut s_eph = my_eph_x.diffie_hellman(their_eph_x_pub).to_bytes();
        let mut ikm = [0u8; 64];
        ikm[..32].copy_from_slice(&s_static);
        ikm[32..].copy_from_slice(&s_eph);
        s_static.zeroize();
        s_eph.zeroize();

        let (low_id, high_id) = if my_peer_id <= their_peer_id {
            (my_peer_id, their_peer_id)
        } else {
            (their_peer_id, my_peer_id)
        };
        let info = format!("AegisMesh-UdpTunnel-v1:{punch_id}:{low_id}:{high_id}");
        let hk = Hkdf::<Sha256>::new(None, &ikm);
        let mut okm = [0u8; 64];
        hk.expand(info.as_bytes(), &mut okm)
            .expect("64-byte HKDF key is valid");
        ikm.zeroize();

        let mut key_low_to_high = [0u8; 32];
        let mut key_high_to_low = [0u8; 32];
        key_low_to_high.copy_from_slice(&okm[..32]);
        key_high_to_low.copy_from_slice(&okm[32..]);
        okm.zeroize();

        if my_peer_id <= their_peer_id {
            Self {
                send_key: key_low_to_high,
                recv_key: key_high_to_low,
            }
        } else {
            Self {
                send_key: key_high_to_low,
                recv_key: key_low_to_high,
            }
        }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<([u8; 12], Vec<u8>)> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.send_key));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad: UDP_TUNNEL_AAD,
                },
            )
            .map_err(|_| AegisError::Crypto("UDP tunnel encryption failed".into()))?;
        Ok((nonce_bytes, ciphertext))
    }

    pub fn decrypt(&self, nonce_bytes: &[u8; 12], ciphertext: &[u8]) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.recv_key));
        cipher
            .decrypt(
                Nonce::from_slice(nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad: UDP_TUNNEL_AAD,
                },
            )
            .map_err(|_| AegisError::Crypto("UDP tunnel decryption failed".into()))
    }
}

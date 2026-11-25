use std::collections::HashMap;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Static};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::error::{AegisError, Result};
use crate::models::SignedProfileCard;

const MAX_SKIPPED_KEYS: usize = 256;

/// Memory-hard Argon2id vault key derivation & authenticated AEAD encryption.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultCipher {
    key_bytes: [u8; 32],
}

impl VaultCipher {
    pub fn derive_from_pin(pin: &str, salt: &[u8; 16]) -> Result<Self> {
        let params = Params::new(16 * 1024, 3, 1, Some(32))
            .map_err(|e| AegisError::Crypto(format!("Argon2 params error: {e}")))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key_bytes = [0u8; 32];
        argon2
            .hash_password_into(pin.as_bytes(), salt, &mut key_bytes)
            .map_err(|e| AegisError::Crypto(format!("Argon2 derivation failed: {e}")))?;
        Ok(Self { key_bytes })
    }

    pub fn from_raw_key(key_bytes: [u8; 32]) -> Self {
        Self { key_bytes }
    }

    pub fn encrypt(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key_bytes));
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|e| AegisError::Crypto(format!("Vault encrypt failed: {e}")))?;

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn decrypt(&self, aad: &[u8], blob: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        if blob.len() < 12 + 16 {
            return Err(AegisError::Crypto("Corrupted vault record".into()));
        }
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key_bytes));
        let nonce = Nonce::from_slice(&blob[..12]);
        let plaintext = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &blob[12..],
                    aad,
                },
            )
            .map_err(|_| AegisError::InvalidCredentials)?;
        Ok(Zeroizing::new(plaintext))
    }
}

/// Wire packet encrypted by the Double Ratchet for end-to-end Forward Secrecy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatchetEnvelope {
    pub dh_pub: [u8; 32],
    pub target_dh_pub: [u8; 32],
    pub prev_chain_len: u32,
    pub msg_num: u32,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

/// Persistent Double Ratchet session state stored inside the encrypted local vault.
/// Uses independent directional root chains (`root_send` / `root_recv`) paired with
/// `target_dh_pub` acknowledgment so simultaneous / offline messages never fork state.
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct DoubleRatchetState {
    root_send: [u8; 32],
    root_recv: [u8; 32],
    send_dh_priv: [u8; 32],
    send_dh_pub: [u8; 32],
    send_target_dh_pub: [u8; 32],
    latest_peer_dh_pub: Option<[u8; 32]>,
    recv_chain_dh_pub: Option<[u8; 32]>,
    send_chain_key: [u8; 32],
    recv_chain_key: [u8; 32],
    send_num: u32,
    recv_num: u32,
    prev_send_len: u32,
    need_dh_step_before_send: bool,
    #[zeroize(skip)]
    my_dh_history: Vec<([u8; 32], [u8; 32])>,
    #[zeroize(skip)]
    skipped_keys: HashMap<String, [u8; 32]>,
}

impl DoubleRatchetState {
    /// Initializes a decentralized Double Ratchet session between two static X25519 keys.
    pub fn initialize(
        my_static_secret: &X25519Static,
        my_static_pub: &X25519Public,
        their_static_pub: &X25519Public,
    ) -> Self {
        let shared = my_static_secret.diffie_hellman(their_static_pub);

        let mut send_info = Vec::with_capacity(64);
        send_info.extend_from_slice(my_static_pub.as_bytes());
        send_info.extend_from_slice(their_static_pub.as_bytes());
        let root_send = hkdf_derive(shared.as_bytes(), &send_info, b"AegisMesh-RootInit-v2");
        let send_chain_key = blake3::derive_key("AegisMesh-ChainInit-v2", &root_send);

        let mut recv_info = Vec::with_capacity(64);
        recv_info.extend_from_slice(their_static_pub.as_bytes());
        recv_info.extend_from_slice(my_static_pub.as_bytes());
        let root_recv = hkdf_derive(shared.as_bytes(), &recv_info, b"AegisMesh-RootInit-v2");
        let recv_chain_key = blake3::derive_key("AegisMesh-ChainInit-v2", &root_recv);

        let mut send_dh_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut send_dh_priv);
        let send_dh_pub = X25519Public::from(&X25519Static::from(send_dh_priv)).to_bytes();

        Self {
            root_send,
            root_recv,
            send_dh_priv,
            send_dh_pub,
            send_target_dh_pub: [0u8; 32],
            latest_peer_dh_pub: None,
            recv_chain_dh_pub: None,
            send_chain_key,
            recv_chain_key,
            send_num: 0,
            recv_num: 0,
            prev_send_len: 0,
            need_dh_step_before_send: false,
            my_dh_history: vec![(send_dh_pub, send_dh_priv)],
            skipped_keys: HashMap::new(),
        }
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<(RatchetEnvelope, u64)> {
        if self.need_dh_step_before_send {
            if let Some(remote_pub_bytes) = self.latest_peer_dh_pub {
                self.dh_ratchet_send(remote_pub_bytes);
            }
            self.need_dh_step_before_send = false;
        }

        let (next_chain, msg_key) = kdf_chain(&self.send_chain_key);
        self.send_chain_key = next_chain;

        let msg_num = self.send_num;
        self.send_num = self.send_num.saturating_add(1);

        let padded = pad_message(plaintext, 128);
        let aad = ratchet_aad(
            &self.send_dh_pub,
            &self.send_target_dh_pub,
            self.prev_send_len,
            msg_num,
        );

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        let cipher = ChaCha20Poly1305::new(Key::from_slice(&msg_key));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &padded,
                    aad: &aad,
                },
            )
            .map_err(|e| AegisError::Crypto(format!("Ratchet encrypt error: {e}")))?;

        let mut zero_msg_key = msg_key;
        zero_msg_key.zeroize();

        Ok((
            RatchetEnvelope {
                dh_pub: self.send_dh_pub,
                target_dh_pub: self.send_target_dh_pub,
                prev_chain_len: self.prev_send_len,
                msg_num,
                nonce: nonce_bytes,
                ciphertext,
            },
            msg_num as u64,
        ))
    }

    pub fn decrypt(&mut self, env: &RatchetEnvelope) -> Result<Zeroizing<Vec<u8>>> {
        // 1. Check skipped out-of-order message keys first
        let skip_id = skipped_key_id(&env.dh_pub, env.msg_num);
        if let Some(mut msg_key) = self.skipped_keys.remove(&skip_id) {
            let res = decrypt_with_key(&msg_key, env);
            msg_key.zeroize();
            return res;
        }

        // Record newest peer ephemeral DH key for our next sending turn
        if self.latest_peer_dh_pub != Some(env.dh_pub) {
            self.latest_peer_dh_pub = Some(env.dh_pub);
            self.need_dh_step_before_send = true;
        }

        // 2. If sender ratcheted against one of our DH keys, mirror that exact DH step
        if env.target_dh_pub != [0u8; 32] && self.recv_chain_dh_pub != Some(env.dh_pub) {
            self.skip_message_keys(env.prev_chain_len)?;
            self.dh_ratchet_recv(env.dh_pub, env.target_dh_pub)?;
        } else if self.recv_chain_dh_pub.is_none() {
            self.recv_chain_dh_pub = Some(env.dh_pub);
        }

        // 3. Advance symmetric receiving chain up to env.msg_num
        self.skip_message_keys(env.msg_num)?;

        let (next_chain, mut msg_key) = kdf_chain(&self.recv_chain_key);
        self.recv_chain_key = next_chain;
        self.recv_num = self.recv_num.saturating_add(1);

        let res = decrypt_with_key(&msg_key, env);
        msg_key.zeroize();
        res
    }

    fn skip_message_keys(&mut self, until: u32) -> Result<()> {
        if until < self.recv_num {
            return Ok(());
        }
        let count = until - self.recv_num;
        if count as usize > MAX_SKIPPED_KEYS {
            return Err(AegisError::Crypto("Too many skipped ratchet messages".into()));
        }
        while self.recv_num < until {
            let (next_chain, msg_key) = kdf_chain(&self.recv_chain_key);
            self.recv_chain_key = next_chain;
            if let Some(dh_pub) = self.recv_chain_dh_pub {
                if self.skipped_keys.len() >= MAX_SKIPPED_KEYS {
                    if let Some(oldest) = self.skipped_keys.keys().next().cloned() {
                        if let Some(mut old_k) = self.skipped_keys.remove(&oldest) {
                            old_k.zeroize();
                        }
                    }
                }
                self.skipped_keys
                    .insert(skipped_key_id(&dh_pub, self.recv_num), msg_key);
            }
            self.recv_num += 1;
        }
        Ok(())
    }

    fn dh_ratchet_recv(&mut self, new_remote_pub: [u8; 32], target_our_pub: [u8; 32]) -> Result<()> {
        let our_priv = self
            .my_dh_history
            .iter()
            .find(|(pub_k, _)| *pub_k == target_our_pub)
            .map(|(_, priv_k)| *priv_k)
            .ok_or_else(|| AegisError::Crypto("Unknown target DH key in ratchet".into()))?;

        self.recv_chain_dh_pub = Some(new_remote_pub);
        self.recv_num = 0;

        let dh_secret =
            X25519Static::from(our_priv).diffie_hellman(&X25519Public::from(new_remote_pub));
        let (new_root, new_recv_chain) = kdf_root(&self.root_recv, dh_secret.as_bytes());
        self.root_recv = new_root;
        self.recv_chain_key = new_recv_chain;
        Ok(())
    }

    fn dh_ratchet_send(&mut self, remote_pub: [u8; 32]) {
        self.prev_send_len = self.send_num;
        self.send_num = 0;

        let mut fresh_priv = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut fresh_priv);
        self.send_dh_priv = fresh_priv;
        self.send_dh_pub = X25519Public::from(&X25519Static::from(fresh_priv)).to_bytes();
        self.send_target_dh_pub = remote_pub;

        self.my_dh_history.push((self.send_dh_pub, self.send_dh_priv));
        if self.my_dh_history.len() > 8 {
            let mut evicted = self.my_dh_history.remove(0);
            evicted.1.zeroize();
        }

        let dh_secret = X25519Static::from(self.send_dh_priv)
            .diffie_hellman(&X25519Public::from(remote_pub));
        let (new_root, new_send_chain) = kdf_root(&self.root_send, dh_secret.as_bytes());
        self.root_send = new_root;
        self.send_chain_key = new_send_chain;
    }
}

/// Sealed anonymous packet for Mesh Store-and-Forward delivery through intermediate peers.
/// Intermediate peers only see `route_tag` and opaque ciphertext; sender & contents stay hidden.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedMeshPacket {
    pub packet_id: String,
    pub route_tag: String,
    pub ephemeral_pub: [u8; 32],
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
    pub hop_ttl: u8,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedInnerPayload {
    pub sender_peer_id: String,
    pub payload_json: Vec<u8>,
    #[serde(default)]
    pub sender_profile: Option<SignedProfileCard>,
}

pub fn compute_route_tag(recipient_peer_id: &str, epoch_salt: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"AegisMesh-RouteTag-v1:");
    hasher.update(recipient_peer_id.as_bytes());
    hasher.update(epoch_salt.as_bytes());
    hex::encode(&hasher.finalize().as_bytes()[..16])
}

pub fn seal_for_recipient(
    packet_id: String,
    recipient_peer_id: &str,
    recipient_x25519_pub: &[u8; 32],
    inner: &SealedInnerPayload,
    hop_ttl: u8,
    expires_at: i64,
) -> Result<SealedMeshPacket> {
    let mut eph_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut eph_bytes);
    let eph_secret = X25519Static::from(eph_bytes);
    eph_bytes.zeroize();
    let eph_pub = X25519Public::from(&eph_secret).to_bytes();

    let shared = eph_secret.diffie_hellman(&X25519Public::from(*recipient_x25519_pub));
    let mut sym_key = hkdf_derive(shared.as_bytes(), &eph_pub, b"AegisMesh-SealedEnvelope-v1");

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let serialized = serde_json::to_vec(inner)?;
    let padded = pad_message(&serialized, 256);

    let cipher = ChaCha20Poly1305::new(Key::from_slice(&sym_key));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: &padded,
                aad: recipient_peer_id.as_bytes(),
            },
        )
        .map_err(|e| AegisError::Crypto(format!("Sealed packet encrypt failed: {e}")))?;

    sym_key.zeroize();

    Ok(SealedMeshPacket {
        packet_id,
        route_tag: compute_route_tag(recipient_peer_id, "global-mesh"),
        ephemeral_pub: eph_pub,
        nonce: nonce_bytes,
        ciphertext,
        hop_ttl,
        expires_at,
    })
}

pub fn unseal_packet(
    packet: &SealedMeshPacket,
    my_peer_id: &str,
    my_x25519_secret: &X25519Static,
) -> Result<SealedInnerPayload> {
    let shared = my_x25519_secret.diffie_hellman(&X25519Public::from(packet.ephemeral_pub));
    let mut sym_key = hkdf_derive(
        shared.as_bytes(),
        &packet.ephemeral_pub,
        b"AegisMesh-SealedEnvelope-v1",
    );

    let cipher = ChaCha20Poly1305::new(Key::from_slice(&sym_key));
    let decrypted = cipher
        .decrypt(
            Nonce::from_slice(&packet.nonce),
            Payload {
                msg: &packet.ciphertext,
                aad: my_peer_id.as_bytes(),
            },
        )
        .map_err(|_| AegisError::Crypto("Unable to unseal mesh packet".into()))?;
    sym_key.zeroize();

    let unpadded = unpad_message(&decrypted)?;
    let inner: SealedInnerPayload = serde_json::from_slice(&unpadded)?;
    Ok(inner)
}

fn decrypt_with_key(msg_key: &[u8; 32], env: &RatchetEnvelope) -> Result<Zeroizing<Vec<u8>>> {
    let aad = ratchet_aad(&env.dh_pub, &env.target_dh_pub, env.prev_chain_len, env.msg_num);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(msg_key));
    let padded = cipher
        .decrypt(
            Nonce::from_slice(&env.nonce),
            Payload {
                msg: &env.ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| AegisError::Crypto("Ratchet decrypt failed".into()))?;
    let plain = unpad_message(&padded)?;
    Ok(Zeroizing::new(plain))
}

fn ratchet_aad(dh_pub: &[u8; 32], target_dh_pub: &[u8; 32], prev_len: u32, msg_num: u32) -> [u8; 72] {
    let mut aad = [0u8; 72];
    aad[..32].copy_from_slice(dh_pub);
    aad[32..64].copy_from_slice(target_dh_pub);
    aad[64..68].copy_from_slice(&prev_len.to_le_bytes());
    aad[68..72].copy_from_slice(&msg_num.to_le_bytes());
    aad
}

fn skipped_key_id(dh_pub: &[u8; 32], msg_num: u32) -> String {
    format!("{}:{}", hex::encode(dh_pub), msg_num)
}

fn hkdf_derive(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm).expect("32-byte HKDF output is valid");
    okm
}

fn kdf_root(root_key: &[u8; 32], dh_out: &[u8]) -> ([u8; 32], [u8; 32]) {
    let hk = Hkdf::<Sha256>::new(Some(root_key), dh_out);
    let mut out = [0u8; 64];
    hk.expand(b"AegisMesh-DR-Step", &mut out)
        .expect("64-byte HKDF output is valid");
    let mut next_root = [0u8; 32];
    let mut chain = [0u8; 32];
    next_root.copy_from_slice(&out[..32]);
    chain.copy_from_slice(&out[32..64]);
    out.zeroize();
    (next_root, chain)
}

fn kdf_chain(chain_key: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let next_chain = blake3::derive_key("AegisMesh-Chain-Next", chain_key);
    let msg_key = blake3::derive_key("AegisMesh-Msg-Key", chain_key);
    (next_chain, msg_key)
}

/// Pads plaintext with ISO 7816-4 (`0x80` followed by `0x00` bytes) up to a multiple of `block_size`
/// to prevent packet-length traffic analysis.
pub fn pad_message(data: &[u8], block_size: usize) -> Vec<u8> {
    let block = block_size.max(16);
    let rem = (data.len() + 1) % block;
    let pad_zeros = if rem == 0 { 0 } else { block - rem };
    let mut out = Vec::with_capacity(data.len() + 1 + pad_zeros);
    out.extend_from_slice(data);
    out.push(0x80);
    out.resize(data.len() + 1 + pad_zeros, 0x00);
    out
}

pub fn unpad_message(padded: &[u8]) -> Result<Vec<u8>> {
    let pos = padded
        .iter()
        .rposition(|&b| b == 0x80)
        .ok_or_else(|| AegisError::Crypto("Invalid padding marker".into()))?;
    if padded[pos + 1..].iter().any(|&b| b != 0x00) {
        return Err(AegisError::Crypto("Corrupted padding bytes".into()));
    }
    Ok(padded[..pos].to_vec())
}

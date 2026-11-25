use std::fs;
use std::path::{Path, PathBuf};

use rand::RngCore;
use redb::{Database, ReadableTable, TableDefinition, TableHandle};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::crypto::{DoubleRatchetState, SealedMeshPacket, VaultCipher};
use crate::dht::SignedDhtRecord;
use crate::error::{AegisError, Result};
use crate::identity::IdentityKeys;
use crate::models::{
    AppSettings, ChatMessage, DhtNodeContact, PeerSummary, SignedProfileCard, VaultUnlockMode,
};

const T_IDENTITY: TableDefinition<&str, &[u8]> = TableDefinition::new("identity");
const T_PEERS: TableDefinition<&str, &[u8]> = TableDefinition::new("peers");
const T_MESSAGES: TableDefinition<&str, &[u8]> = TableDefinition::new("messages");
const T_RATCHETS: TableDefinition<&str, &[u8]> = TableDefinition::new("ratchets");
const T_OUTBOX: TableDefinition<&str, &[u8]> = TableDefinition::new("outbox");
const T_MESH_CACHE: TableDefinition<&str, &[u8]> = TableDefinition::new("mesh_cache");
const T_DHT_CONTACTS: TableDefinition<&str, &[u8]> = TableDefinition::new("dht_contacts");
const T_DHT_RECORDS: TableDefinition<&str, &[u8]> = TableDefinition::new("dht_records");
const T_SETTINGS: TableDefinition<&str, &[u8]> = TableDefinition::new("settings");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuressAction {
    Decoy,
    Wipe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultMetadata {
    version: u8,
    primary_salt: [u8; 16],
    primary_check: Vec<u8>,
    duress_salt: Option<[u8; 16]>,
    duress_check: Option<Vec<u8>>,
    duress_action: Option<DuressAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredOutboxEntry {
    pub msg_id: String,
    pub recipient_id: String,
    pub sealed: SealedMeshPacket,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxItem {
    pub msg_id: String,
    pub recipient_id: String,
    pub sealed: SealedMeshPacket,
    pub created_at: i64,
}

/// Canonicalizes user PIN input by trimming leading/trailing whitespace
/// and normalizing Persian ('۰'..='۹') and Arabic-Indic ('٠'..='٩') numerals to ASCII '0'..='9'.
pub fn normalize_pin(pin: &str) -> String {
    pin.trim()
        .chars()
        .map(|c| match c {
            '۰' => '0',
            '۱' => '1',
            '۲' => '2',
            '۳' => '3',
            '۴' => '4',
            '۵' => '5',
            '۶' => '6',
            '۷' => '7',
            '۸' => '8',
            '۹' => '9',
            '٠' => '0',
            '١' => '1',
            '٢' => '2',
            '٣' => '3',
            '٤' => '4',
            '٥' => '5',
            '٦' => '6',
            '٧' => '7',
            '٨' => '8',
            '٩' => '9',
            other => other,
        })
        .collect()
}

pub struct UnlockedVault {
    pub mode: VaultUnlockMode,
    pub data_dir: PathBuf,
    db: Database,
    cipher: VaultCipher,
}

impl UnlockedVault {
    pub fn metadata_path(data_dir: &Path) -> PathBuf {
        data_dir.join("vault.meta.json")
    }

    pub fn is_initialized(data_dir: &Path) -> bool {
        Self::metadata_path(data_dir).exists()
    }

    /// Securely wipes all vault metadata and database files so the user can
    /// recover from a 12-word mnemonic or initialize a new identity after a forgotten PIN.
    pub fn wipe_all(data_dir: &Path) {
        secure_shred_file(&Self::metadata_path(data_dir));
        secure_shred_file(&data_dir.join("primary.redb"));
        secure_shred_file(&data_dir.join("decoy.redb"));
    }

    /// Creates a brand new encrypted vault with a Primary PIN and optional Panic/Duress PIN.
    pub fn create_new(
        data_dir: &Path,
        pin: &str,
        keys: &IdentityKeys,
        profile: &SignedProfileCard,
        duress_pin: Option<(&str, DuressAction)>,
    ) -> Result<Self> {
        Self::create_new_with_settings(
            data_dir, pin, keys, profile, duress_pin, &AppSettings::default(),
        )
    }

    pub(crate) fn create_new_with_settings(
        data_dir: &Path,
        pin: &str,
        keys: &IdentityKeys,
        profile: &SignedProfileCard,
        duress_pin: Option<(&str, DuressAction)>,
        settings: &AppSettings,
    ) -> Result<Self> {
        fs::create_dir_all(data_dir)?;
        let meta_path = Self::metadata_path(data_dir);
        if meta_path.exists() {
            return Err(AegisError::VaultAlreadyInitialized);
        }

        let norm_pin = normalize_pin(pin);
        let mut primary_salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut primary_salt);
        let primary_cipher = VaultCipher::derive_from_pin(&norm_pin, &primary_salt)?;
        let primary_check = primary_cipher.encrypt(b"vault-check:primary", b"AEGIS_OK_V1")?;

        let (duress_salt, duress_check, duress_action) = if let Some((d_pin, action)) = duress_pin {
            let norm_d = normalize_pin(d_pin);
            if norm_d == norm_pin {
                return Err(AegisError::InvalidCredentials);
            }
            let mut d_salt = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut d_salt);
            let d_cipher = VaultCipher::derive_from_pin(&norm_d, &d_salt)?;
            let d_check = d_cipher.encrypt(b"vault-check:duress", b"AEGIS_DURESS_V1")?;
            (Some(d_salt), Some(d_check), Some(action))
        } else {
            (None, None, None)
        };

        let meta = VaultMetadata {
            version: 1,
            primary_salt,
            primary_check,
            duress_salt,
            duress_check,
            duress_action,
        };
        fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?)?;

        let primary_db = open_redb(&data_dir.join("primary.redb"))?;
        let vault = Self {
            mode: VaultUnlockMode::Primary,
            data_dir: data_dir.to_path_buf(),
            db: primary_db,
            cipher: primary_cipher,
        };
        vault.save_identity_seed(&keys.master_seed)?;
        vault.save_own_profile(profile)?;
        vault.save_settings(settings)?;

        // Prepare empty decoy database if decoy duress mode was configured
        if let (Some(d_salt), Some(DuressAction::Decoy), Some((d_pin, _))) =
            (meta.duress_salt, meta.duress_action, duress_pin)
        {
            let d_cipher = VaultCipher::derive_from_pin(d_pin, &d_salt)?;
            let decoy_db = open_redb(&data_dir.join("decoy.redb"))?;
            let (decoy_keys, _) = IdentityKeys::generate()?;
            let decoy_profile = decoy_keys.build_signed_profile(
                "Guest Node",
                "@guest",
                "Local node ready.",
                "Observer",
                "#00F0FF",
                None,
                crate::models::PresenceStatus::Online,
            );
            let decoy_vault = Self {
                mode: VaultUnlockMode::Decoy,
                data_dir: data_dir.to_path_buf(),
                db: decoy_db,
                cipher: d_cipher,
            };
            decoy_vault.save_identity_seed(&decoy_keys.master_seed)?;
            decoy_vault.save_own_profile(&decoy_profile)?;
            decoy_vault.save_settings(&AppSettings::default())?;
        }

        Ok(vault)
    }

    /// Unlocks either the primary vault or triggers the configured Duress/Panic mode.
    pub fn unlock(data_dir: &Path, pin: &str) -> Result<(Self, IdentityKeys)> {
        let meta_path = Self::metadata_path(data_dir);
        if !meta_path.exists() {
            return Err(AegisError::VaultNotInitialized);
        }
        let raw_meta = fs::read(&meta_path)?;
        let mut meta: VaultMetadata = serde_json::from_slice(&raw_meta)?;

        let mut candidates = Vec::new();
        let norm = normalize_pin(pin);
        candidates.push(norm.clone());
        let trimmed = pin.trim().to_string();
        if trimmed != norm {
            candidates.push(trimmed);
        }
        if pin != norm && pin != pin.trim() {
            candidates.push(pin.to_string());
        }

        // Try primary PIN candidates first
        for cand in &candidates {
            if let Ok(primary_cipher) = VaultCipher::derive_from_pin(cand, &meta.primary_salt) {
                if let Ok(plain) = primary_cipher.decrypt(b"vault-check:primary", &meta.primary_check) {
                    if plain.as_slice() == b"AEGIS_OK_V1" {
                        let db = open_redb(&data_dir.join("primary.redb"))?;
                        let vault = Self {
                            mode: VaultUnlockMode::Primary,
                            data_dir: data_dir.to_path_buf(),
                            db,
                            cipher: primary_cipher,
                        };
                        let seed = vault.load_identity_seed()?;
                        let keys = IdentityKeys::from_master_seed(seed);
                        return Ok((vault, keys));
                    }
                }
            }
        }

        // Check Duress / Panic PIN candidates
        if let (Some(d_salt), Some(d_check), Some(action)) =
            (meta.duress_salt, meta.duress_check.as_ref(), meta.duress_action)
        {
            for cand in &candidates {
                if let Ok(d_cipher) = VaultCipher::derive_from_pin(cand, &d_salt) {
                    if let Ok(plain) = d_cipher.decrypt(b"vault-check:duress", d_check) {
                        if plain.as_slice() == b"AEGIS_DURESS_V1" {
                            match action {
                                DuressAction::Decoy => {
                                    let db = open_redb(&data_dir.join("decoy.redb"))?;
                                    let vault = Self {
                                        mode: VaultUnlockMode::Decoy,
                                        data_dir: data_dir.to_path_buf(),
                                        db,
                                        cipher: d_cipher,
                                    };
                                    let seed = vault.load_identity_seed()?;
                                    let keys = IdentityKeys::from_master_seed(seed);
                                    return Ok((vault, keys));
                                }
                                DuressAction::Wipe => {
                                    // Cryptographically overwrite and remove primary vault immediately
                                    secure_shred_file(&data_dir.join("primary.redb"));
                                    secure_shred_file(&data_dir.join("decoy.redb"));
                                    let (fresh_keys, _) = IdentityKeys::generate()?;
                                    let fresh_profile = fresh_keys.build_signed_profile(
                                        "Local User",
                                        "@local",
                                        "",
                                        "Node",
                                        "#00F0FF",
                                        None,
                                        crate::models::PresenceStatus::Online,
                                    );

                                    let mut new_salt = [0u8; 16];
                                    rand::thread_rng().fill_bytes(&mut new_salt);
                                    let new_cipher = VaultCipher::derive_from_pin(cand, &new_salt)?;
                                    let new_check = new_cipher.encrypt(b"vault-check:primary", b"AEGIS_OK_V1")?;
                                    meta.primary_salt = new_salt;
                                    meta.primary_check = new_check;
                                    meta.duress_salt = None;
                                    meta.duress_check = None;
                                    meta.duress_action = None;
                                    fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?)?;

                                    let db = open_redb(&data_dir.join("primary.redb"))?;
                                    let vault = Self {
                                        mode: VaultUnlockMode::Wiped,
                                        data_dir: data_dir.to_path_buf(),
                                        db,
                                        cipher: new_cipher,
                                    };
                                    vault.save_identity_seed(&fresh_keys.master_seed)?;
                                    vault.save_own_profile(&fresh_profile)?;
                                    vault.save_settings(&AppSettings::default())?;
                                    return Ok((vault, fresh_keys));
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(AegisError::InvalidCredentials)
    }

    /// Updates or configures the Panic/Duress PIN on an already unlocked primary vault.
    pub fn configure_duress_pin(&self, duress_pin: &str, action: DuressAction) -> Result<()> {
        if self.mode != VaultUnlockMode::Primary {
            return Err(AegisError::InvalidCredentials);
        }
        let norm_d = normalize_pin(duress_pin);
        let meta_path = Self::metadata_path(&self.data_dir);
        let raw_meta = fs::read(&meta_path)?;
        let mut meta: VaultMetadata = serde_json::from_slice(&raw_meta)?;

        let mut d_salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut d_salt);
        let d_cipher = VaultCipher::derive_from_pin(&norm_d, &d_salt)?;
        let d_check = d_cipher.encrypt(b"vault-check:duress", b"AEGIS_DURESS_V1")?;

        meta.duress_salt = Some(d_salt);
        meta.duress_check = Some(d_check);
        meta.duress_action = Some(action);
        fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?)?;

        if action == DuressAction::Decoy {
            let decoy_path = self.data_dir.join("decoy.redb");
            let _ = fs::remove_file(&decoy_path);
            let decoy_db = open_redb(&decoy_path)?;
            let (decoy_keys, _) = IdentityKeys::generate()?;
            let decoy_profile = decoy_keys.build_signed_profile(
                "Guest Node",
                "@guest",
                "Local node ready.",
                "Observer",
                "#00F0FF",
                None,
                crate::models::PresenceStatus::Online,
            );
            let decoy_vault = Self {
                mode: VaultUnlockMode::Decoy,
                data_dir: self.data_dir.clone(),
                db: decoy_db,
                cipher: d_cipher,
            };
            decoy_vault.save_identity_seed(&decoy_keys.master_seed)?;
            decoy_vault.save_own_profile(&decoy_profile)?;
            decoy_vault.save_settings(&AppSettings::default())?;
        }
        Ok(())
    }

    pub fn save_identity_seed(&self, seed: &[u8; 32]) -> Result<()> {
        self.put_encrypted(T_IDENTITY, "master_seed", seed)
    }

    pub fn load_identity_seed(&self) -> Result<[u8; 32]> {
        let mut bytes = self
            .get_decrypted(T_IDENTITY, "master_seed")?
            .ok_or_else(|| AegisError::Storage("Missing master seed".into()))?;
        let arr: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| AegisError::Storage("Invalid master seed length".into()))?;
        bytes.zeroize();
        Ok(arr)
    }

    pub fn save_own_profile(&self, profile: &SignedProfileCard) -> Result<()> {
        let bytes = serde_json::to_vec(profile)?;
        self.put_encrypted(T_IDENTITY, "own_profile", &bytes)
    }

    pub fn load_own_profile(&self) -> Result<SignedProfileCard> {
        let bytes = self
            .get_decrypted(T_IDENTITY, "own_profile")?
            .ok_or_else(|| AegisError::Storage("Missing own profile".into()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let bytes = serde_json::to_vec(settings)?;
        self.put_encrypted(T_SETTINGS, "app_settings", &bytes)
    }

    pub fn load_settings(&self) -> Result<AppSettings> {
        match self.get_decrypted(T_SETTINGS, "app_settings")? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(AppSettings::default()),
        }
    }

    pub fn save_peer(&self, peer: &PeerSummary) -> Result<()> {
        let bytes = serde_json::to_vec(peer)?;
        self.put_encrypted(T_PEERS, &peer.profile.peer_id, &bytes)
    }

    pub fn load_all_peers(&self) -> Result<Vec<PeerSummary>> {
        let records = self.scan_decrypted(T_PEERS)?;
        let mut peers: Vec<PeerSummary> = Vec::with_capacity(records.len());
        for (_, val) in records {
            peers.push(serde_json::from_slice(&val)?);
        }
        peers.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.last_message_at.unwrap_or(0).cmp(&a.last_message_at.unwrap_or(0)))
                .then_with(|| b.last_seen.cmp(&a.last_seen))
        });
        Ok(peers)
    }

    pub fn save_message(&self, msg: &ChatMessage) -> Result<()> {
        let key = format!("{}:{:020}:{}", msg.chat_id, msg.timestamp, msg.id);
        let bytes = serde_json::to_vec(msg)?;
        self.put_encrypted(T_MESSAGES, &key, &bytes)
    }

    pub fn load_messages(&self, chat_id: &str) -> Result<Vec<ChatMessage>> {
        let prefix = format!("{chat_id}:");
        let records = self.scan_decrypted(T_MESSAGES)?;
        let mut msgs: Vec<ChatMessage> = Vec::new();
        for (k, val) in records {
            if k.starts_with(&prefix) {
                msgs.push(serde_json::from_slice(&val)?);
            }
        }
        msgs.sort_by_key(|m| m.timestamp);
        Ok(msgs)
    }

    pub fn delete_message(&self, chat_id: &str, message_id: &str) -> Result<bool> {
        let prefix = format!("{chat_id}:");
        let suffix = format!(":{message_id}");
        let keys = self.list_keys(T_MESSAGES)?;
        let mut removed = false;
        for k in keys {
            if k.starts_with(&prefix) && k.ends_with(&suffix) {
                self.remove_key(T_MESSAGES, &k)?;
                removed = true;
            }
        }
        Ok(removed)
    }

    pub fn purge_expired_messages(&self, now_ms: i64) -> Result<Vec<(String, String)>> {
        let records = self.scan_decrypted(T_MESSAGES)?;
        let mut expired = Vec::new();
        for (k, val) in records {
            if let Ok(msg) = serde_json::from_slice::<ChatMessage>(&val) {
                if let Some(exp) = msg.expires_at {
                    if now_ms >= exp {
                        self.remove_key(T_MESSAGES, &k)?;
                        expired.push((msg.chat_id, msg.id));
                    }
                }
            }
        }
        Ok(expired)
    }

    pub fn save_ratchet_state(&self, peer_id: &str, state: &DoubleRatchetState) -> Result<()> {
        let mut bytes = serde_json::to_vec(state)?;
        let res = self.put_encrypted(T_RATCHETS, peer_id, &bytes);
        bytes.zeroize();
        res
    }

    pub fn load_ratchet_state(&self, peer_id: &str) -> Result<Option<DoubleRatchetState>> {
        match self.get_decrypted(T_RATCHETS, peer_id)? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn enqueue_outbox(&self, item: &OutboxItem) -> Result<()> {
        let entry = StoredOutboxEntry {
            msg_id: item.msg_id.clone(),
            recipient_id: item.recipient_id.clone(),
            sealed: item.sealed.clone(),
            created_at: item.created_at,
        };
        let bytes = serde_json::to_vec(&entry)?;
        self.put_encrypted(T_OUTBOX, &item.msg_id, &bytes)
    }

    pub fn load_outbox(&self) -> Result<Vec<OutboxItem>> {
        let records = self.scan_decrypted(T_OUTBOX)?;
        let mut out = Vec::with_capacity(records.len());
        for (_, val) in records {
            let entry: StoredOutboxEntry = serde_json::from_slice(&val)?;
            out.push(OutboxItem {
                msg_id: entry.msg_id,
                recipient_id: entry.recipient_id,
                sealed: entry.sealed,
                created_at: entry.created_at,
            });
        }
        out.sort_by_key(|i| i.created_at);
        Ok(out)
    }

    pub fn remove_outbox(&self, msg_id: &str) -> Result<()> {
        self.remove_key(T_OUTBOX, msg_id)
    }

    /// Removes a peer contact, its message history, ratchet session, and queued outbox items.
    pub fn delete_chat(&self, peer_id: &str) -> Result<()> {
        let _ = self.remove_key(T_PEERS, peer_id);
        let _ = self.remove_key(T_RATCHETS, peer_id);

        let prefix = format!("{peer_id}:");
        for key in self.list_keys(T_MESSAGES)? {
            if key.starts_with(&prefix) {
                let _ = self.remove_key(T_MESSAGES, &key);
            }
        }

        for item in self.load_outbox()? {
            if item.recipient_id == peer_id {
                let _ = self.remove_key(T_OUTBOX, &item.msg_id);
            }
        }

        Ok(())
    }

    pub fn save_mesh_relay_packet(&self, packet: &SealedMeshPacket) -> Result<()> {
        let bytes = serde_json::to_vec(packet)?;
        self.put_encrypted(T_MESH_CACHE, &packet.packet_id, &bytes)
    }

    pub fn load_mesh_relay_packets(&self, now_ms: i64) -> Result<Vec<SealedMeshPacket>> {
        let records = self.scan_decrypted(T_MESH_CACHE)?;
        let mut valid = Vec::new();
        for (k, val) in records {
            if let Ok(pkt) = serde_json::from_slice::<SealedMeshPacket>(&val) {
                if pkt.expires_at > now_ms && pkt.hop_ttl > 0 {
                    valid.push(pkt);
                } else {
                    let _ = self.remove_key(T_MESH_CACHE, &k);
                }
            }
        }
        Ok(valid)
    }

    pub fn remove_mesh_relay_packet(&self, packet_id: &str) -> Result<()> {
        self.remove_key(T_MESH_CACHE, packet_id)
    }

    pub fn save_dht_contacts(&self, contacts: &[DhtNodeContact]) -> Result<()> {
        let bytes = serde_json::to_vec(contacts)?;
        self.put_encrypted(T_DHT_CONTACTS, "routing_table_v2", &bytes)
    }

    pub fn load_dht_contacts(&self) -> Result<Vec<DhtNodeContact>> {
        match self.get_decrypted(T_DHT_CONTACTS, "routing_table_v2")? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(Vec::new()),
        }
    }

    pub fn save_dht_record(&self, record: &SignedDhtRecord) -> Result<()> {
        let bytes = serde_json::to_vec(record)?;
        self.put_encrypted(T_DHT_RECORDS, &record.peer_id, &bytes)
    }

    pub fn load_dht_records(&self, now_ms: i64) -> Result<Vec<SignedDhtRecord>> {
        let records = self.scan_decrypted(T_DHT_RECORDS)?;
        let mut out = Vec::new();
        for (k, val) in records {
            if let Ok(rec) = serde_json::from_slice::<SignedDhtRecord>(&val) {
                if rec.verify(now_ms).is_ok() {
                    out.push(rec);
                } else {
                    let _ = self.remove_key(T_DHT_RECORDS, &k);
                }
            }
        }
        Ok(out)
    }

    fn put_encrypted(&self, table_def: TableDefinition<&str, &[u8]>, key: &str, plaintext: &[u8]) -> Result<()> {
        let aad = format!("{}:{}", table_def.name(), key);
        let blob = self.cipher.encrypt(aad.as_bytes(), plaintext)?;
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| AegisError::Storage(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(table_def)
                .map_err(|e| AegisError::Storage(e.to_string()))?;
            table
                .insert(key, blob.as_slice())
                .map_err(|e| AegisError::Storage(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| AegisError::Storage(e.to_string()))?;
        Ok(())
    }

    fn get_decrypted(&self, table_def: TableDefinition<&str, &[u8]>, key: &str) -> Result<Option<Vec<u8>>> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| AegisError::Storage(e.to_string()))?;
        let table = match read_txn.open_table(table_def) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(AegisError::Storage(e.to_string())),
        };
        let Some(guard) = table.get(key).map_err(|e| AegisError::Storage(e.to_string()))? else {
            return Ok(None);
        };
        let aad = format!("{}:{}", table_def.name(), key);
        let plain = self.cipher.decrypt(aad.as_bytes(), guard.value())?;
        Ok(Some(plain.to_vec()))
    }

    fn scan_decrypted(&self, table_def: TableDefinition<&str, &[u8]>) -> Result<Vec<(String, Vec<u8>)>> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| AegisError::Storage(e.to_string()))?;
        let table = match read_txn.open_table(table_def) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(AegisError::Storage(e.to_string())),
        };
        let mut results = Vec::new();
        for item in table.iter().map_err(|e| AegisError::Storage(e.to_string()))? {
            let (k_guard, v_guard) = item.map_err(|e| AegisError::Storage(e.to_string()))?;
            let key = k_guard.value().to_string();
            let aad = format!("{}:{}", table_def.name(), key);
            let plain = self.cipher.decrypt(aad.as_bytes(), v_guard.value())?;
            results.push((key, plain.to_vec()));
        }
        Ok(results)
    }

    fn list_keys(&self, table_def: TableDefinition<&str, &[u8]>) -> Result<Vec<String>> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| AegisError::Storage(e.to_string()))?;
        let table = match read_txn.open_table(table_def) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(AegisError::Storage(e.to_string())),
        };
        let mut keys = Vec::new();
        for item in table.iter().map_err(|e| AegisError::Storage(e.to_string()))? {
            let (k_guard, _) = item.map_err(|e| AegisError::Storage(e.to_string()))?;
            keys.push(k_guard.value().to_string());
        }
        Ok(keys)
    }

    fn remove_key(&self, table_def: TableDefinition<&str, &[u8]>, key: &str) -> Result<()> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| AegisError::Storage(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(table_def)
                .map_err(|e| AegisError::Storage(e.to_string()))?;
            table
                .remove(key)
                .map_err(|e| AegisError::Storage(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| AegisError::Storage(e.to_string()))?;
        Ok(())
    }
}

fn open_redb(path: &Path) -> Result<Database> {
    let db = Database::create(path).map_err(|e| AegisError::Storage(e.to_string()))?;
    // Ensure all tables exist upfront
    let write_txn = db.begin_write().map_err(|e| AegisError::Storage(e.to_string()))?;
    {
        let _ = write_txn.open_table(T_IDENTITY).map_err(|e| AegisError::Storage(e.to_string()))?;
        let _ = write_txn.open_table(T_PEERS).map_err(|e| AegisError::Storage(e.to_string()))?;
        let _ = write_txn.open_table(T_MESSAGES).map_err(|e| AegisError::Storage(e.to_string()))?;
        let _ = write_txn.open_table(T_RATCHETS).map_err(|e| AegisError::Storage(e.to_string()))?;
        let _ = write_txn.open_table(T_OUTBOX).map_err(|e| AegisError::Storage(e.to_string()))?;
        let _ = write_txn.open_table(T_MESH_CACHE).map_err(|e| AegisError::Storage(e.to_string()))?;
        let _ = write_txn.open_table(T_SETTINGS).map_err(|e| AegisError::Storage(e.to_string()))?;
    }
    write_txn.commit().map_err(|e| AegisError::Storage(e.to_string()))?;
    Ok(db)
}

fn secure_shred_file(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let len = meta.len() as usize;
        if len > 0 {
            let mut zeros = vec![0u8; len.min(1024 * 1024)];
            rand::thread_rng().fill_bytes(&mut zeros);
            let _ = fs::write(path, &zeros);
        }
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_pin() {
        assert_eq!(normalize_pin("  ۱۲۳۴  "), "1234");
        assert_eq!(normalize_pin("١٢٣٤٥٦٧٨٩٠"), "1234567890");
        assert_eq!(normalize_pin("۰۱۲۳۴۵۶۷۸۹"), "0123456789");
        assert_eq!(normalize_pin("  1234  "), "1234");
        assert_eq!(normalize_pin("pass-۱۲۳"), "pass-123");
    }

    #[test]
    fn test_persian_digit_vault_cross_unlock() {
        let temp = tempfile::tempdir().unwrap();
        let (keys, _) = crate::identity::IdentityKeys::generate().unwrap();
        let profile = keys.build_signed_profile("Test", "@test", "", "Node", "#00F0FF", None, crate::models::PresenceStatus::Online);

        // Created with Persian numerals and whitespace
        let vault = UnlockedVault::create_new(temp.path(), "  ۱۲۳۴  ", &keys, &profile, None).unwrap();
        drop(vault);

        // Unlocked with standard English digits
        let (unlocked, loaded_keys) = UnlockedVault::unlock(temp.path(), "1234").unwrap();
        assert_eq!(loaded_keys.peer_id(), keys.peer_id());
        drop(unlocked);

        // Unlocked with Arabic-Indic digits
        let (unlocked_ar, _) = UnlockedVault::unlock(temp.path(), "١٢٣٤").unwrap();
        assert_eq!(unlocked_ar.mode, VaultUnlockMode::Primary);
    }
}

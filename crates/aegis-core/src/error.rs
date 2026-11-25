use thiserror::Error;

#[derive(Debug, Error)]
pub enum AegisError {
    #[error("Vault is locked")]
    VaultLocked,

    #[error("Vault already exists at path")]
    VaultAlreadyInitialized,

    #[error("Vault is not initialized yet")]
    VaultNotInitialized,

    #[error("Invalid PIN or passphrase")]
    InvalidCredentials,

    #[error("Cryptographic operation failed: {0}")]
    Crypto(String),

    #[error("Signature verification failed for peer {0}")]
    InvalidSignature(String),

    #[error("Malformed key or identifier: {0}")]
    InvalidKey(String),

    #[error("Invalid profile payload: {0}")]
    InvalidPayload(String),

    #[error("Invalid recovery mnemonic: {0}")]
    InvalidMnemonic(String),

    #[error("Database storage error: {0}")]
    Storage(String),

    #[error("Network transport error: {0}")]
    Network(String),

    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AegisError>;

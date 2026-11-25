use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Static};

use crate::crypto::{pad_message, unpad_message, RatchetEnvelope, SealedMeshPacket};
use crate::dht::{OnionPacket, SignedDhtRecord};
use crate::error::{AegisError, Result};
use crate::identity::decode_32_b58;
use crate::models::{DeliveryStatus, DhtNodeContact, SignedProfileCard, VpnAdapterInfo};

pub const DEFAULT_DISCOVERY_PORT: u16 = 47832;
pub const DEFAULT_TCP_PORT: u16 = 47833;
pub const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 77, 88);
const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const MAX_FRAME_BYTES: usize = 512 * 1024;
const NOISE_CHUNK_MAX: usize = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryBeacon {
    pub v: u8,
    pub profile: SignedProfileCard,
    pub tcp_port: u16,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChannelAttestation {
    profile: SignedProfileCard,
    /// Ed25519 signature over b"AegisMesh-NoiseBind-v1:" || noise_handshake_hash
    binding_sig: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum WireFrame {
    ProfileSync(SignedProfileCard),
    DirectMessage {
        msg_id: String,
        reply_to_id: Option<String>,
        expires_in_secs: Option<u64>,
        timestamp: i64,
        envelope: RatchetEnvelope,
    },
    DeliveryReceipt {
        msg_id: String,
        status: DeliveryStatus,
    },
    Typing {
        is_typing: bool,
    },
    Reaction {
        msg_id: String,
        emoji: String,
        sender_id: String,
    },
    EditMessage {
        msg_id: String,
        new_content: String,
    },
    DeleteMessage {
        msg_id: String,
    },
    SealedRelay(SealedMeshPacket),
    /// Peer-observed external endpoint notice (replaces centralized STUN servers)
    PeerObservedAddr {
        observed_ip: String,
        observed_tcp_port: u16,
        peer_udp_port: u16,
        peer_accepts_relay: bool,
    },
    /// Kademlia XOR DHT iterative discovery & Exchange (PEX)
    DhtFindNode {
        req_id: String,
        target_peer_id: String,
    },
    DhtFindNodeResp {
        req_id: String,
        contacts: Vec<DhtNodeContact>,
    },
    /// Signed peer reachability advertisement
    DhtAnnounceRecord(SignedDhtRecord),
    /// Explicit withdrawal / departure notification from DHT
    DhtWithdrawRecord {
        peer_id: String,
        signature: String,
    },
    DhtQueryRecord {
        req_id: String,
        target_peer_id: String,
    },
    DhtQueryRecordResp {
        req_id: String,
        record: Option<SignedDhtRecord>,
        closer_contacts: Vec<DhtNodeContact>,
    },
    /// Multi-hop decentralized @handle / Account ID search across the Cymesh overlay
    DhtSearchAccount {
        req_id: String,
        query: String,
        ttl: u8,
    },
    DhtSearchAccountResp {
        req_id: String,
        records: Vec<SignedDhtRecord>,
    },
    /// Pull queued encrypted SealedMeshPackets stored on DHT neighbors while offline
    DhtFetchMailbox {
        peer_id: String,
    },
    /// Multi-hop blinded onion relay packet
    OnionRelay(OnionPacket),
    /// Coordinated UDP NAT Hole Punching signal via mutual mesh relay
    PunchCoordinate {
        punch_id: String,
        initiator_peer_id: String,
        initiator_x25519_pub: String,
        initiator_udp_addr: String,
        target_peer_id: String,
    },
    PunchReady {
        punch_id: String,
        responder_peer_id: String,
        responder_x25519_pub: String,
        responder_udp_addr: String,
        initiator_peer_id: String,
    },
    Ping(i64),
    Pong(i64),
}

pub fn enumerate_local_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let IpAddr::V4(v4) = iface.ip() {
                if !v4.is_link_local() {
                    ips.push(v4);
                }
            }
        }
    }
    ips
}

pub fn enumerate_broadcast_addrs() -> Vec<Ipv4Addr> {
    let mut addrs = vec![Ipv4Addr::BROADCAST, MULTICAST_ADDR];
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4_if) = iface.addr {
                let ip = u32::from(v4_if.ip);
                let mask = u32::from(v4_if.netmask);
                let bcast = Ipv4Addr::from(ip | !mask);
                if !addrs.contains(&bcast) {
                    addrs.push(bcast);
                }
            }
        }
    }
    addrs
}

/// Detects if the host system has active VPN / TUN / TAP / Wireguard network adapters.
pub fn check_local_vpn_adapters() -> VpnAdapterInfo {
    let mut detected_adapter: Option<String> = None;
    let vpn_keywords = [
        "tun", "tap", "wg", "wintun", "wireguard", "windscribe", "openvpn",
        "v2ray", "xray", "sing-box", "singbox", "tun2socks", "warp", "cloudflare",
        "tailscale", "cisco", "nord", "proton", "surfshark", "express", "outline",
        "clash", "nekoray", "nekobox", "psiphon", "anyconnect", "forticlient",
        "zerotier", "hamachi"
    ];

    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            let name_lower = iface.name.to_lowercase();
            for kw in &vpn_keywords {
                if name_lower.contains(kw) {
                    detected_adapter = Some(iface.name.clone());
                    break;
                }
            }
            if detected_adapter.is_some() {
                break;
            }
        }
    }

    let mut proxy_detected = false;
    let mut proxy_details: Option<String> = None;
    for var in &["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        if let Ok(val) = std::env::var(var) {
            let trimmed = val.trim();
            // Don't flag empty or typical local loopback
            if !trimmed.is_empty() {
                proxy_detected = true;
                proxy_details = Some(format!("{var}={trimmed}"));
                break;
            }
        }
    }

    VpnAdapterInfo {
        vpn_detected: detected_adapter.is_some(),
        adapter_name: detected_adapter,
        proxy_detected,
        proxy_details,
    }
}

pub fn bind_discovery_socket(port: u16) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    {
        let _ = socket.set_reuse_port(true);
    }
    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;

    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    if let Err(err) = socket.bind(&addr.into()) {
        // Fallback to ephemeral port if port is strictly locked on a single host test
        tracing::warn!("Could not bind discovery port {port} ({err}); falling back to ephemeral");
        let fallback = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
        socket.bind(&fallback.into())?;
    }

    let _ = socket.join_multicast_v4(&MULTICAST_ADDR, &Ipv4Addr::UNSPECIFIED);
    let _ = socket.set_multicast_loop_v4(true);

    let std_sock: StdUdpSocket = socket.into();
    UdpSocket::from_std(std_sock).map_err(AegisError::Io)
}

pub async fn bind_tcp_listener(preferred_port: u16) -> Result<(TcpListener, u16)> {
    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, preferred_port));
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(_) => TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await?,
    };
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

pub fn bind_mesh_udp_socket(preferred_port: u16) -> Result<(UdpSocket, u16)> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;

    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, preferred_port));
    if socket.bind(&addr.into()).is_err() {
        let fallback = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
        socket.bind(&fallback.into())?;
    }

    let std_sock: StdUdpSocket = socket.into();
    let tokio_sock = UdpSocket::from_std(std_sock)?;
    let port = tokio_sock.local_addr()?.port();
    Ok((tokio_sock, port))
}

pub fn is_private_or_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 0b0100_0000)
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
    }
}

pub struct NoiseChannel {
    stream: TcpStream,
    transport: snow::TransportState,
    pub peer_profile: SignedProfileCard,
}

impl NoiseChannel {
    pub async fn accept(
        mut stream: TcpStream,
        my_x25519: &X25519Static,
        my_signing: &SigningKey,
        my_profile: &SignedProfileCard,
    ) -> Result<Self> {
        let _ = stream.set_nodelay(true);
        let builder = snow::Builder::new(NOISE_PARAMS.parse().map_err(|e| AegisError::Crypto(format!("{e}")))?);
        let mut hs = builder
            .local_private_key(my_x25519.as_bytes())
            .build_responder()
            .map_err(|e| AegisError::Crypto(format!("Noise responder init failed: {e}")))?;

        let mut buf = vec![0u8; 65535];

        // <- e
        let msg1 = read_raw_packet(&mut stream).await?;
        hs.read_message(&msg1, &mut buf)
            .map_err(|e| AegisError::Crypto(format!("Noise msg1 failed: {e}")))?;

        // -> e, ee, s, es
        let len2 = hs
            .write_message(&[], &mut buf)
            .map_err(|e| AegisError::Crypto(format!("Noise msg2 failed: {e}")))?;
        write_raw_packet(&mut stream, &buf[..len2]).await?;

        // <- s, se
        let msg3 = read_raw_packet(&mut stream).await?;
        hs.read_message(&msg3, &mut buf)
            .map_err(|e| AegisError::Crypto(format!("Noise msg3 failed: {e}")))?;

        let remote_x = hs
            .get_remote_static()
            .ok_or_else(|| AegisError::Crypto("Missing remote Noise static key".into()))?
            .to_vec();
        let hs_hash = hs.get_handshake_hash().to_vec();

        let mut transport = hs
            .into_transport_mode()
            .map_err(|e| AegisError::Crypto(format!("Noise transport transition failed: {e}")))?;

        // Exchange cryptographic channel binding attestation
        let my_attest = build_attestation(my_profile, my_signing, &hs_hash)?;
        send_encrypted_payload(&mut stream, &mut transport, &serde_json::to_vec(&my_attest)?).await?;

        let peer_bytes = recv_encrypted_payload(&mut stream, &mut transport).await?;
        let peer_attest: ChannelAttestation = serde_json::from_slice(&peer_bytes)?;
        verify_attestation(&peer_attest, &remote_x, &hs_hash)?;

        Ok(Self {
            stream,
            transport,
            peer_profile: peer_attest.profile,
        })
    }

    pub async fn connect(
        addr: SocketAddr,
        my_x25519: &X25519Static,
        my_signing: &SigningKey,
        my_profile: &SignedProfileCard,
    ) -> Result<Self> {
        let mut stream = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(addr))
            .await
            .map_err(|_| AegisError::Network(format!("Connect timeout to {addr}")))??;
        let _ = stream.set_nodelay(true);

        let builder = snow::Builder::new(NOISE_PARAMS.parse().map_err(|e| AegisError::Crypto(format!("{e}")))?);
        let mut hs = builder
            .local_private_key(my_x25519.as_bytes())
            .build_initiator()
            .map_err(|e| AegisError::Crypto(format!("Noise initiator init failed: {e}")))?;

        let mut buf = vec![0u8; 65535];

        // -> e
        let len1 = hs
            .write_message(&[], &mut buf)
            .map_err(|e| AegisError::Crypto(format!("Noise msg1 write failed: {e}")))?;
        write_raw_packet(&mut stream, &buf[..len1]).await?;

        // <- e, ee, s, es
        let msg2 = read_raw_packet(&mut stream).await?;
        hs.read_message(&msg2, &mut buf)
            .map_err(|e| AegisError::Crypto(format!("Noise msg2 read failed: {e}")))?;

        // -> s, se
        let len3 = hs
            .write_message(&[], &mut buf)
            .map_err(|e| AegisError::Crypto(format!("Noise msg3 write failed: {e}")))?;
        write_raw_packet(&mut stream, &buf[..len3]).await?;

        let remote_x = hs
            .get_remote_static()
            .ok_or_else(|| AegisError::Crypto("Missing remote Noise static key".into()))?
            .to_vec();
        let hs_hash = hs.get_handshake_hash().to_vec();

        let mut transport = hs
            .into_transport_mode()
            .map_err(|e| AegisError::Crypto(format!("Noise transport transition failed: {e}")))?;

        let peer_bytes = recv_encrypted_payload(&mut stream, &mut transport).await?;
        let peer_attest: ChannelAttestation = serde_json::from_slice(&peer_bytes)?;
        verify_attestation(&peer_attest, &remote_x, &hs_hash)?;

        let my_attest = build_attestation(my_profile, my_signing, &hs_hash)?;
        send_encrypted_payload(&mut stream, &mut transport, &serde_json::to_vec(&my_attest)?).await?;

        Ok(Self {
            stream,
            transport,
            peer_profile: peer_attest.profile,
        })
    }

    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.stream.peer_addr().ok()
    }

    pub fn split(self) -> (NoiseReader, NoiseWriter, SignedProfileCard) {
        let (read_half, write_half) = self.stream.into_split();
        let transport = Arc::new(Mutex::new(self.transport));
        (
            NoiseReader {
                stream: read_half,
                transport: transport.clone(),
            },
            NoiseWriter {
                stream: write_half,
                transport,
            },
            self.peer_profile,
        )
    }
}

pub struct NoiseReader {
    stream: OwnedReadHalf,
    transport: Arc<Mutex<snow::TransportState>>,
}

pub struct NoiseWriter {
    stream: OwnedWriteHalf,
    transport: Arc<Mutex<snow::TransportState>>,
}

impl NoiseReader {
    pub async fn recv_frame(&mut self) -> Result<WireFrame> {
        let first_pkt = read_raw_packet_half(&mut self.stream).await?;
        let mut pt_buf = vec![0u8; 65535];
        let n = {
            let mut guard = self.transport.lock().map_err(|_| AegisError::Crypto("Noise lock poisoned".into()))?;
            guard
                .read_message(&first_pkt, &mut pt_buf)
                .map_err(|e| AegisError::Crypto(format!("Noise decrypt failed: {e}")))?
        };
        if n < 4 {
            return Err(AegisError::Network("Truncated Noise payload header".into()));
        }
        let total_len = u32::from_le_bytes([pt_buf[0], pt_buf[1], pt_buf[2], pt_buf[3]]) as usize;
        if total_len > MAX_FRAME_BYTES {
            return Err(AegisError::Network("Incoming frame too large".into()));
        }

        let mut acc = Vec::with_capacity(total_len);
        acc.extend_from_slice(&pt_buf[4..n]);
        while acc.len() < total_len {
            let pkt = read_raw_packet_half(&mut self.stream).await?;
            let m = {
                let mut guard = self.transport.lock().map_err(|_| AegisError::Crypto("Noise lock poisoned".into()))?;
                guard
                    .read_message(&pkt, &mut pt_buf)
                    .map_err(|e| AegisError::Crypto(format!("Noise chunk decrypt failed: {e}")))?
            };
            acc.extend_from_slice(&pt_buf[..m]);
        }
        acc.truncate(total_len);
        let raw = unpad_message(&acc)?;
        Ok(serde_json::from_slice(&raw)?)
    }
}

impl NoiseWriter {
    pub async fn send_frame(&mut self, frame: &WireFrame) -> Result<()> {
        let raw = serde_json::to_vec(frame)?;
        let padded = pad_message(&raw, 256);
        if padded.len() > MAX_FRAME_BYTES {
            return Err(AegisError::Network("Payload exceeds maximum frame size".into()));
        }

        let total_len = padded.len() as u32;
        let mut header_and_data = Vec::with_capacity(4 + padded.len());
        header_and_data.extend_from_slice(&total_len.to_le_bytes());
        header_and_data.extend_from_slice(&padded);

        let encrypted_chunks = {
            let mut guard = self.transport.lock().map_err(|_| AegisError::Crypto("Noise lock poisoned".into()))?;
            let mut ct_buf = vec![0u8; 65535];
            let mut chunks = Vec::new();
            for chunk in header_and_data.chunks(NOISE_CHUNK_MAX) {
                let n = guard
                    .write_message(chunk, &mut ct_buf)
                    .map_err(|e| AegisError::Crypto(format!("Noise encrypt failed: {e}")))?;
                chunks.push(ct_buf[..n].to_vec());
            }
            chunks
        };

        for pkt in encrypted_chunks {
            write_raw_packet_half(&mut self.stream, &pkt).await?;
        }
        Ok(())
    }
}

async fn write_raw_packet_half(stream: &mut OwnedWriteHalf, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(data).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_raw_packet_half(stream: &mut OwnedReadHalf) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 65535 {
        return Err(AegisError::Network(format!("Invalid packet size: {len}")));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

fn build_attestation(
    profile: &SignedProfileCard,
    signing_key: &SigningKey,
    hs_hash: &[u8],
) -> Result<ChannelAttestation> {
    let mut msg = b"AegisMesh-NoiseBind-v1:".to_vec();
    msg.extend_from_slice(hs_hash);
    let sig: Signature = signing_key.sign(&msg);
    Ok(ChannelAttestation {
        profile: profile.clone(),
        binding_sig: bs58::encode(sig.to_bytes()).into_string(),
    })
}

fn verify_attestation(
    attest: &ChannelAttestation,
    remote_noise_static: &[u8],
    hs_hash: &[u8],
) -> Result<()> {
    attest.profile.verify()?;

    let x_bytes = decode_32_b58(&attest.profile.x25519_pub)?;
    if x_bytes.as_slice() != remote_noise_static {
        return Err(AegisError::InvalidSignature(
            "X25519 static key mismatch with Noise session".into(),
        ));
    }

    let ed_bytes = decode_32_b58(&attest.profile.peer_id)?;
    let verifying_key =
        VerifyingKey::from_bytes(&ed_bytes).map_err(|e| AegisError::InvalidKey(e.to_string()))?;

    let sig_vec = bs58::decode(&attest.binding_sig)
        .into_vec()
        .map_err(|_| AegisError::InvalidSignature(attest.profile.peer_id.clone()))?;
    let sig_arr: [u8; 64] = sig_vec
        .try_into()
        .map_err(|_| AegisError::InvalidSignature(attest.profile.peer_id.clone()))?;
    let sig = Signature::from_bytes(&sig_arr);

    let mut msg = b"AegisMesh-NoiseBind-v1:".to_vec();
    msg.extend_from_slice(hs_hash);
    verifying_key
        .verify(&msg, &sig)
        .map_err(|_| AegisError::InvalidSignature(attest.profile.peer_id.clone()))?;

    let _ = X25519Public::from(x_bytes);
    Ok(())
}

async fn send_encrypted_payload(
    stream: &mut TcpStream,
    transport: &mut snow::TransportState,
    plaintext: &[u8],
) -> Result<()> {
    if plaintext.len() > MAX_FRAME_BYTES {
        return Err(AegisError::Network("Payload exceeds maximum frame size".into()));
    }
    // Split large payloads into Noise-safe chunks <= NOISE_CHUNK_MAX
    let total_len = plaintext.len() as u32;
    let mut header_and_data = Vec::with_capacity(4 + plaintext.len());
    header_and_data.extend_from_slice(&total_len.to_le_bytes());
    header_and_data.extend_from_slice(plaintext);

    let mut ct_buf = vec![0u8; 65535];
    for chunk in header_and_data.chunks(NOISE_CHUNK_MAX) {
        let n = transport
            .write_message(chunk, &mut ct_buf)
            .map_err(|e| AegisError::Crypto(format!("Noise encrypt failed: {e}")))?;
        write_raw_packet(stream, &ct_buf[..n]).await?;
    }
    Ok(())
}

async fn recv_encrypted_payload(
    stream: &mut TcpStream,
    transport: &mut snow::TransportState,
) -> Result<Vec<u8>> {
    let mut pt_buf = vec![0u8; 65535];
    let first_pkt = read_raw_packet(stream).await?;
    let n = transport
        .read_message(&first_pkt, &mut pt_buf)
        .map_err(|e| AegisError::Crypto(format!("Noise decrypt failed: {e}")))?;
    if n < 4 {
        return Err(AegisError::Network("Truncated Noise payload header".into()));
    }
    let total_len = u32::from_le_bytes([pt_buf[0], pt_buf[1], pt_buf[2], pt_buf[3]]) as usize;
    if total_len > MAX_FRAME_BYTES {
        return Err(AegisError::Network("Incoming frame too large".into()));
    }

    let mut acc = Vec::with_capacity(total_len);
    acc.extend_from_slice(&pt_buf[4..n]);
    while acc.len() < total_len {
        let pkt = read_raw_packet(stream).await?;
        let m = transport
            .read_message(&pkt, &mut pt_buf)
            .map_err(|e| AegisError::Crypto(format!("Noise chunk decrypt failed: {e}")))?;
        acc.extend_from_slice(&pt_buf[..m]);
    }
    acc.truncate(total_len);
    Ok(acc)
}

async fn write_raw_packet(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(data).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_raw_packet(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 65535 {
        return Err(AegisError::Network(format!("Invalid packet size: {len}")));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

export type PresenceStatus = "online" | "stealth" | "busy" | "away";
export type ConnectionRoute = "lan_direct" | "wan_direct" | "mesh_relay" | "offline";
export type DeliveryStatus =
  | "queued_offline"
  | "relayed_mesh"
  | "sent_direct"
  | "delivered"
  | "read"
  | "failed";
export type VaultUnlockMode = "primary" | "decoy" | "wiped";
export type DuressAction = "decoy" | "wipe";

export interface VaultStatusInfo {
  initialized: boolean;
  unlocked: boolean;
  unlock_mode: VaultUnlockMode | null;
  platform: string;
}

export interface SignedProfileCard {
  peer_id: string;
  x25519_pub: string;
  display_name: string;
  handle: string;
  bio: string;
  role_badge: string;
  avatar_color: string;
  avatar_data: string | null;
  status: PresenceStatus;
  updated_at: number;
  signature: string;
}

export interface MessageReaction {
  emoji: string;
  sender_id: string;
}

export interface ChatMessage {
  id: string;
  chat_id: string;
  sender_id: string;
  recipient_id: string;
  content: string;
  reply_to_id: string | null;
  reactions: MessageReaction[];
  timestamp: number;
  expires_at: number | null;
  status: DeliveryStatus;
  edited: boolean;
  ratchet_counter: number;
}

export interface PeerSummary {
  profile: SignedProfileCard;
  account_id: string;
  fingerprint: string;
  route: ConnectionRoute;
  latency_ms: number | null;
  verified: boolean;
  safety_number: string;
  safety_emojis: string[];
  last_addr: string | null;
  last_seen: number;
  unread_count: number;
  last_message_preview: string | null;
  last_message_at: number | null;
  pinned: boolean;
}

export interface DhtNodeContact {
  peer_id: string;
  x25519_pub: string;
  tcp_addrs: string[];
  udp_addr: string | null;
  relay_capable: boolean;
  last_seen: number;
  failures: number;
}

export interface AppSettings {
  language: string;
  stealth_mode: boolean;
  mesh_relay_enabled: boolean;
  listen_port: number;
  auto_lock_minutes: number;
  default_ephemeral_secs: number | null;
  dht_enabled: boolean;
  hole_punch_enabled: boolean;
  onion_routing_enabled: boolean;
  bootstrap_peers: string[];
  enforce_iran_ip?: boolean;
}

export interface VpnAdapterInfo {
  vpn_detected: boolean;
  adapter_name: string | null;
  proxy_detected: boolean;
  proxy_details: string | null;
}

export interface NetworkTelemetry {
  node_listening_port: number;
  local_ips: string[];
  share_uri: string;
  active_direct_peers: number;
  queued_offline_messages: number;
  mesh_relayed_packets: number;
  stealth_mode: boolean;
  observed_external_addr: string | null;
  account_id: string;
  dht_known_nodes: number;
  dht_stored_records: number;
  active_mesh_relays: number;
  nat_punch_successes: number;
  onion_routed_packets: number;
}

export interface IdentityCreatedBundle {
  profile: SignedProfileCard;
  account_id: string;
  recovery_mnemonic: string;
  fingerprint: string;
  share_uri: string;
}

export interface AccountSearchResult {
  peer_id: string;
  account_id: string;
  profile: SignedProfileCard;
  direct_reachable: boolean;
  relay_hops_available: number;
  already_paired: boolean;
}

export interface VolunteerRelayResult {
  addr: string;
  peer_id: string;
  latency_ms: number;
  propagated_peers: number;
}

export interface ServerHealthResult {
  target: string;
  reachable: boolean;
  latency_ms?: number;
  noise_handshake_ok: boolean;
  peer_id?: string;
  handle?: string;
  display_name?: string;
  role?: string;
  observed_ip?: string;
  message: string;
  timestamp: number;
}

export type CoreEvent =
  | { type: "PeerUpdated"; payload: PeerSummary }
  | { type: "ChatDeleted"; payload: { peer_id: string } }
  | { type: "MessageReceived"; payload: ChatMessage }
  | { type: "MessageUpdated"; payload: ChatMessage }
  | { type: "MessageDeleted"; payload: { chat_id: string; message_id: string } }
  | { type: "TypingStatus"; payload: { peer_id: string; is_typing: boolean } }
  | { type: "NetworkStatus"; payload: NetworkTelemetry }
  | { type: "VaultLocked" };

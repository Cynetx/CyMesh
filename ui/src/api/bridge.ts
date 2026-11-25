import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AccountSearchResult,
  AppSettings,
  ChatMessage,
  CoreEvent,
  DhtNodeContact,
  DuressAction,
  IdentityCreatedBundle,
  NetworkTelemetry,
  PeerSummary,
  PresenceStatus,
  ServerHealthResult,
  SignedProfileCard,
  VaultStatusInfo,
  VaultUnlockMode,
  VolunteerRelayResult,
  VpnAdapterInfo,
} from "../types/aegis";

export const api = {
  getVaultStatus: () => invoke<VaultStatusInfo>("get_vault_status"),

  createIdentityAndVault: (params: {
    pin: string;
    displayName: string;
    handle: string;
    bio: string;
    roleBadge: string;
    avatarColor: string;
    duressPin?: string | null;
    duressAction?: DuressAction | null;
  }) =>
    invoke<IdentityCreatedBundle>("create_identity_and_vault", {
      pin: params.pin,
      displayName: params.displayName,
      handle: params.handle,
      bio: params.bio,
      roleBadge: params.roleBadge,
      avatarColor: params.avatarColor,
      duressPin: params.duressPin ?? null,
      duressAction: params.duressAction ?? null,
    }),

  recoverIdentityAndVault: (params: {
    mnemonic: string;
    pin: string;
    displayName: string;
    handle: string;
    bio: string;
    roleBadge: string;
    avatarColor: string;
  }) =>
    invoke<IdentityCreatedBundle>("recover_identity_and_vault", {
      mnemonic: params.mnemonic,
      pin: params.pin,
      displayName: params.displayName,
      handle: params.handle,
      bio: params.bio,
      roleBadge: params.roleBadge,
      avatarColor: params.avatarColor,
    }),

  unlockVault: (pin: string) => invoke<VaultUnlockMode>("unlock_vault", { pin }),
  lockVault: () => invoke<void>("lock_vault"),
  resetVault: () => invoke<void>("reset_vault"),
  configureDuressPin: (duressPin: string, action: DuressAction) =>
    invoke<void>("configure_duress_pin", { duressPin, action }),

  getOwnProfile: () => invoke<SignedProfileCard>("get_own_profile"),
  updateOwnProfile: (params: {
    displayName: string;
    handle: string;
    bio: string;
    roleBadge: string;
    avatarColor: string;
    avatarData: string | null;
    status: PresenceStatus;
  }) =>
    invoke<SignedProfileCard>("update_own_profile", {
      displayName: params.displayName,
      handle: params.handle,
      bio: params.bio,
      roleBadge: params.roleBadge,
      avatarColor: params.avatarColor,
      avatarData: params.avatarData,
      status: params.status,
    }),

  getSettings: () => invoke<AppSettings>("get_settings"),
  updateSettings: (settings: AppSettings) =>
    invoke<void>("update_settings", { settings }),

  listPeers: () => invoke<PeerSummary[]>("list_peers"),
  listMessages: (chatId: string) =>
    invoke<ChatMessage[]>("list_messages", { chatId }),
  deleteChat: (peerId: string) => invoke<void>("delete_chat", { peerId }),

  addPeerByUri: (uri: string) => invoke<PeerSummary>("add_peer_by_uri", { uri }),
  connectPeerAddr: (addr: string) =>
    invoke<PeerSummary>("connect_peer_addr", { addr }),

  sendMessage: (params: {
    peerId: string;
    content: string;
    replyToId?: string | null;
    expiresInSecs?: number | null;
  }) =>
    invoke<ChatMessage>("send_message", {
      peerId: params.peerId,
      content: params.content,
      replyToId: params.replyToId ?? null,
      expiresInSecs: params.expiresInSecs ?? null,
    }),

  sendTyping: (peerId: string, isTyping: boolean) =>
    invoke<void>("send_typing", { peerId, isTyping }),

  toggleReaction: (chatId: string, msgId: string, emoji: string) =>
    invoke<void>("toggle_reaction", { chatId, msgId, emoji }),

  editMessage: (chatId: string, msgId: string, newContent: string) =>
    invoke<void>("edit_message", { chatId, msgId, newContent }),

  deleteMessage: (chatId: string, msgId: string, forBoth: boolean) =>
    invoke<void>("delete_message", { chatId, msgId, forBoth }),

  markChatRead: (peerId: string) => invoke<void>("mark_chat_read", { peerId }),

  setPeerVerified: (peerId: string, verified: boolean) =>
    invoke<PeerSummary>("set_peer_verified", { peerId, verified }),

  setPeerPinned: (peerId: string, pinned: boolean) =>
    invoke<PeerSummary>("set_peer_pinned", { peerId, pinned }),

  getNetworkTelemetry: () => invoke<NetworkTelemetry>("get_network_telemetry"),
  triggerDiscoveryPulse: () => invoke<void>("trigger_discovery_pulse"),
  installWindowsShortcuts: () => invoke<string>("install_windows_shortcuts"),

  connectPeerById: (peerId: string) =>
    invoke<PeerSummary>("connect_peer_by_id", { peerId }),
  searchAccounts: (query: string) =>
    invoke<AccountSearchResult[]>("search_accounts", { query }),
  connectByAccount: (identifier: string) =>
    invoke<PeerSummary>("connect_by_account", { identifier }),
  initiateUdpPunch: (peerId: string) =>
    invoke<PeerSummary>("initiate_udp_punch", { peerId }),
  addBootstrapPeer: (target: string) =>
    invoke<string[]>("add_bootstrap_peer", { target }),
  removeBootstrapPeer: (target: string) =>
    invoke<string[]>("remove_bootstrap_peer", { target }),
  registerVolunteerRelay: (target: string) =>
    invoke<VolunteerRelayResult>("register_volunteer_relay", { target }),
  testServerHealth: (target: string) =>
    invoke<ServerHealthResult>("test_server_health", { target }),
  checkVpnAdapters: () => invoke<VpnAdapterInfo>("check_vpn_adapters"),
  listDhtContacts: () => invoke<DhtNodeContact[]>("list_dht_contacts"),
  exportSeedBundle: () => invoke<string>("export_seed_bundle"),
  importSeedBundle: (bundle: string) =>
    invoke<number>("import_seed_bundle", { bundle }),

  onCoreEvent: (handler: (evt: CoreEvent) => void): Promise<UnlistenFn> =>
    listen<CoreEvent>("aegis-event", (event) => handler(event.payload)),
};

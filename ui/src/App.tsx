import React, { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./api/bridge";
import { ChatList } from "./components/ChatList";
import { ChatPane } from "./components/ChatPane";
import { LockScreen } from "./components/LockScreen";
import { OnboardingModal } from "./components/OnboardingModal";
import { PairModal } from "./components/PairModal";
import { ProfileDrawer } from "./components/ProfileDrawer";
import { SettingsModal } from "./components/SettingsModal";
import { SidebarNav, type ChatFilter } from "./components/SidebarNav";
import { VpnGuardBlockScreen } from "./components/VpnGuardBlockScreen";
import { MeshStartup } from "./components/MeshStartup";
import { dictionaries, getDirection, type AppLang } from "./i18n";
import { evaluateVpnGuard, type VpnGuardStatus } from "./utils/vpnGuard";
import { isDirectRoute } from "./utils/route";
import type {
  AppSettings,
  ChatMessage,
  NetworkTelemetry,
  PeerSummary,
  SignedProfileCard,
  VaultStatusInfo,
} from "./types/aegis";

export const App: React.FC = () => {
  const [vaultStatus, setVaultStatus] = useState<VaultStatusInfo | null>(null);
  const [lang, setLang] = useState<AppLang>("fa");
  const [ownProfile, setOwnProfile] = useState<SignedProfileCard | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [peers, setPeers] = useState<PeerSummary[]>([]);
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null);
  const [messagesByChat, setMessagesByChat] = useState<Record<string, ChatMessage[]>>({});
  const [typingPeers, setTypingPeers] = useState<Record<string, boolean>>({});
  const [telemetry, setTelemetry] = useState<NetworkTelemetry | null>(null);

  const [filter, setFilter] = useState<ChatFilter>("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [showProfileDrawer, setShowProfileDrawer] = useState(false);
  const [showPairModal, setShowPairModal] = useState(false);
  const [pairInitialQuery, setPairInitialQuery] = useState("");
  const [showSettingsModal, setShowSettingsModal] = useState(false);
  const [bootingUplink, setBootingUplink] = useState(false);
  const [onboardingInitialMode, setOnboardingInitialMode] = useState<"create" | "recover">("create");
  const [vpnGuardStatus, setVpnGuardStatus] = useState<VpnGuardStatus | null>(null);

  const isHiddenRelayPeer = (p: PeerSummary) =>
    p.profile.role_badge.toLowerCase() === "relay node";

  const t = dictionaries[lang];

  useEffect(() => {
    document.documentElement.lang = lang;
    document.documentElement.dir = getDirection(lang);
  }, [lang]);

  const checkVpn = useCallback(async () => {
    const enforce = settings?.enforce_iran_ip ?? false;
    try {
      const res = await evaluateVpnGuard(enforce);
      setVpnGuardStatus(res);
      return res;
    } catch (err) {
      console.warn("VPN Guard evaluation failed:", err);
      return { blocked: false, checkedAt: Date.now() };
    }
  }, [settings?.enforce_iran_ip]);

  useEffect(() => {
    void checkVpn();
    const interval = setInterval(() => {
      void checkVpn();
    }, 12000);
    return () => clearInterval(interval);
  }, [checkVpn]);

  const hydrateSession = useCallback(async (withBootUplink = false) => {
    const status = await api.getVaultStatus();
    setVaultStatus(status);
    if (!status.unlocked) return;
    if (withBootUplink) {
      setBootingUplink(true);
    }

    const [prof, cfg, peerList, tel] = await Promise.all([
      api.getOwnProfile(),
      api.getSettings(),
      api.listPeers(),
      api.getNetworkTelemetry(),
    ]);
    setOwnProfile(prof);
    setSettings(cfg);
    if (cfg.language === "en" || cfg.language === "fa") {
      setLang(cfg.language);
    }
    setPeers(peerList);
    setTelemetry(tel);
    if (peerList.length > 0 && !selectedPeerId) {
      setSelectedPeerId(peerList[0].profile.peer_id);
    }
  }, [selectedPeerId]);

  useEffect(() => {
    void hydrateSession(true);
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "l" && vaultStatus?.unlocked) {
        e.preventDefault();
        void api.lockVault();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [vaultStatus?.unlocked]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    api
      .onCoreEvent((evt) => {
        switch (evt.type) {
          case "PeerUpdated": {
            const updated = evt.payload;
            if (isHiddenRelayPeer(updated)) break;
            setPeers((prev) => {
              const rest = prev.filter((p) => p.profile.peer_id !== updated.profile.peer_id);
              return [updated, ...rest].sort((a, b) => {
                if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
                return (b.last_message_at ?? b.last_seen) - (a.last_message_at ?? a.last_seen);
              });
            });
            break;
          }
          case "ChatDeleted": {
            const { peer_id } = evt.payload;
            setPeers((prev) => prev.filter((p) => p.profile.peer_id !== peer_id));
            setMessagesByChat((prev) => {
              const next = { ...prev };
              delete next[peer_id];
              return next;
            });
            setSelectedPeerId((cur) => (cur === peer_id ? null : cur));
            break;
          }
          case "MessageReceived": {
            const msg = evt.payload;
            setMessagesByChat((prev) => {
              const existing = prev[msg.chat_id] ?? [];
              if (existing.some((m) => m.id === msg.id)) return prev;
              return { ...prev, [msg.chat_id]: [...existing, msg] };
            });
            if (msg.chat_id === selectedPeerId && msg.sender_id === selectedPeerId) {
              void api.markChatRead(selectedPeerId);
            }
            break;
          }
          case "MessageUpdated": {
            const updated = evt.payload;
            setMessagesByChat((prev) => {
              const existing = prev[updated.chat_id] ?? [];
              return {
                ...prev,
                [updated.chat_id]: existing.map((m) => (m.id === updated.id ? updated : m)),
              };
            });
            break;
          }
          case "MessageDeleted": {
            const { chat_id, message_id } = evt.payload;
            setMessagesByChat((prev) => ({
              ...prev,
              [chat_id]: (prev[chat_id] ?? []).filter((m) => m.id !== message_id),
            }));
            break;
          }
          case "TypingStatus": {
            const { peer_id, is_typing } = evt.payload;
            setTypingPeers((prev) => ({ ...prev, [peer_id]: is_typing }));
            break;
          }
          case "NetworkStatus": {
            setTelemetry(evt.payload);
            break;
          }
          case "VaultLocked": {
            setOwnProfile(null);
            setPeers([]);
            setMessagesByChat({});
            setVaultStatus((v) => (v ? { ...v, unlocked: false } : v));
            break;
          }
        }
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      if (unlisten) unlisten();
    };
  }, [selectedPeerId]);

  useEffect(() => {
    if (!selectedPeerId || !vaultStatus?.unlocked) return;
    void api.listMessages(selectedPeerId).then((msgs) => {
      setMessagesByChat((prev) => ({ ...prev, [selectedPeerId]: msgs }));
    });
    void api.markChatRead(selectedPeerId);
  }, [selectedPeerId, vaultStatus?.unlocked]);

  const handleToggleLang = async () => {
    const next: AppLang = lang === "fa" ? "en" : "fa";
    setLang(next);
    if (settings && vaultStatus?.unlocked) {
      const updated = { ...settings, language: next };
      setSettings(updated);
      await api.updateSettings(updated);
    }
  };

  const handleToggleStealth = async () => {
    if (!settings) return;
    const next = { ...settings, stealth_mode: !settings.stealth_mode };
    await api.updateSettings(next);
    setSettings(next);
  };

  const filteredPeers = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return peers.filter((p) => {
      if (isHiddenRelayPeer(p)) return false;
      if (filter === "direct" && !isDirectRoute(p.route)) return false;
      if (filter === "mesh" && isDirectRoute(p.route)) return false;
      if (filter === "verified" && !p.verified) return false;
      if (!q) return true;
      return (
        p.profile.display_name.toLowerCase().includes(q) ||
        p.profile.handle.toLowerCase().includes(q) ||
        (p.account_id ?? "").toLowerCase().includes(q) ||
        p.fingerprint.toLowerCase().includes(q)
      );
    });
  }, [peers, filter, searchQuery]);

  if (!vaultStatus) {
    return (
      <div
        className="mono"
        style={{
          height: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "#00F0FF",
          fontWeight: 700,
        }}
      >
        CYMESH // INIT...
      </div>
    );
  }

  if (vpnGuardStatus?.blocked) {
    return (
      <VpnGuardBlockScreen
        guardStatus={vpnGuardStatus}
        onRetry={async () => {
          await checkVpn();
        }}
        lang={lang}
      />
    );
  }

  if (!vaultStatus.initialized) {
    return (
      <OnboardingModal
        t={t}
        lang={lang}
        initialMode={onboardingInitialMode}
        onToggleLang={() => void handleToggleLang()}
        onComplete={() => {
          setOnboardingInitialMode("create");
          return hydrateSession(true);
        }}
      />
    );
  }

  if (!vaultStatus.unlocked) {
    return (
      <LockScreen
        t={t}
        lang={lang}
        onToggleLang={() => void handleToggleLang()}
        onUnlock={async (pin) => {
          await api.unlockVault(pin);
          await hydrateSession(true);
        }}
        onResetVault={async (openRecover) => {
          await api.resetVault();
          setOnboardingInitialMode(openRecover ? "recover" : "create");
          await hydrateSession(false);
        }}
      />
    );
  }

  if (bootingUplink) {
    return (
      <MeshStartup
        t={t}
        accountId={telemetry?.account_id ?? null}
        onFinish={() => setBootingUplink(false)}
      />
    );
  }

  const selectedPeer =
    peers.find((p) => p.profile.peer_id === selectedPeerId) ?? null;
  const activeMessages = selectedPeerId ? messagesByChat[selectedPeerId] ?? [] : [];

  const openPairModal = (initialQ?: string) => {
    void api.getNetworkTelemetry().then(setTelemetry);
    setPairInitialQuery(initialQ ?? "");
    setShowPairModal(true);
  };

  return (
    <div className="workspace-shell">
      <SidebarNav
        t={t}
        lang={lang}
        ownProfile={ownProfile}
        filter={filter}
        stealthMode={settings?.stealth_mode ?? false}
        onChangeFilter={setFilter}
        onToggleStealth={() => void handleToggleStealth()}
        onOpenPair={() => openPairModal()}
        onOpenSettings={() => {
          void api.getNetworkTelemetry().then(setTelemetry);
          setShowSettingsModal(true);
        }}
        onToggleLang={() => void handleToggleLang()}
        onLockVault={() => void api.lockVault()}
      />

      <ChatList
        t={t}
        lang={lang}
        peers={filteredPeers}
        selectedPeerId={selectedPeerId}
        searchQuery={searchQuery}
        typingPeers={typingPeers}
        onSearchChange={setSearchQuery}
        onSelectPeer={setSelectedPeerId}
        onTogglePinPeer={async (peerId, pinned) => {
          await api.setPeerPinned(peerId, pinned);
        }}
        onDeletePeer={async (peerId) => {
          await api.deleteChat(peerId);
        }}
        onScanPulse={() => void api.triggerDiscoveryPulse()}
        onOpenPair={(initialQ) => openPairModal(initialQ)}
      />

      <ChatPane
        t={t}
        lang={lang}
        myPeerId={ownProfile?.peer_id ?? ""}
        peer={selectedPeer}
        messages={activeMessages}
        isTyping={selectedPeer ? Boolean(typingPeers[selectedPeer.profile.peer_id]) : false}
        showProfileDrawer={showProfileDrawer}
        onToggleProfileDrawer={() => setShowProfileDrawer((v) => !v)}
        onSendMessage={async (content, replyToId, expiresInSecs) => {
          if (!selectedPeer) return;
          await api.sendMessage({
            peerId: selectedPeer.profile.peer_id,
            content,
            replyToId,
            expiresInSecs,
          });
        }}
        onSendTyping={(isTyping) => {
          if (!selectedPeer) return;
          void api.sendTyping(selectedPeer.profile.peer_id, isTyping);
        }}
        onToggleReaction={async (msgId, emoji) => {
          if (!selectedPeer) return;
          await api.toggleReaction(selectedPeer.profile.peer_id, msgId, emoji);
        }}
        onEditMessage={async (msgId, newContent) => {
          if (!selectedPeer) return;
          await api.editMessage(selectedPeer.profile.peer_id, msgId, newContent);
        }}
        onDeleteMessage={async (msgId, forBoth) => {
          if (!selectedPeer) return;
          await api.deleteMessage(selectedPeer.profile.peer_id, msgId, forBoth);
        }}
      />

      {selectedPeer && showProfileDrawer && (
        <>
          <div
            className="drawer-backdrop"
            onClick={() => setShowProfileDrawer(false)}
          />
          <ProfileDrawer
            t={t}
            peer={selectedPeer}
            onClose={() => setShowProfileDrawer(false)}
            onToggleVerify={async (verified) => {
              await api.setPeerVerified(selectedPeer.profile.peer_id, verified);
            }}
            onTogglePin={async (pinned) => {
              await api.setPeerPinned(selectedPeer.profile.peer_id, pinned);
            }}
            onDeleteChat={async () => {
              await api.deleteChat(selectedPeer.profile.peer_id);
            }}
          />
        </>
      )}

      {showPairModal && (
        <PairModal
          t={t}
          telemetry={telemetry}
          initialQuery={pairInitialQuery}
          onClose={() => {
            setShowPairModal(false);
            setPairInitialQuery("");
          }}
          onPeerAdded={(id) => setSelectedPeerId(id)}
        />
      )}

      {showSettingsModal && ownProfile && settings && (
        <SettingsModal
          t={t}
          platform={vaultStatus.platform}
          profile={ownProfile}
          settings={settings}
          telemetry={telemetry}
          onClose={() => setShowSettingsModal(false)}
          onProfileUpdated={setOwnProfile}
          onSettingsUpdated={setSettings}
        />
      )}
    </div>
  );
};

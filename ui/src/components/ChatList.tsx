import React, { useEffect, useState } from "react";
import { Globe, Pin, RefreshCw, Search, Trash2, UserPlus } from "lucide-react";
import { formatTime, type AppLang } from "../i18n";
import type { TranslationDict } from "../i18n/en";
import type { PeerSummary } from "../types/aegis";
import { formatRouteLabel, isOnlineRoute, routePillClass } from "../utils/route";
import { CyberAvatar } from "./CyberAvatar";

interface ChatListProps {
  t: TranslationDict;
  lang: AppLang;
  peers: PeerSummary[];
  selectedPeerId: string | null;
  searchQuery: string;
  typingPeers: Record<string, boolean>;
  onSearchChange: (q: string) => void;
  onSelectPeer: (peerId: string) => void;
  onTogglePinPeer: (peerId: string, pinned: boolean) => Promise<void>;
  onDeletePeer: (peerId: string) => Promise<void>;
  onScanPulse: () => void;
  onOpenPair: (initialQuery?: string) => void;
}

export const ChatList: React.FC<ChatListProps> = ({
  t,
  lang,
  peers,
  selectedPeerId,
  searchQuery,
  typingPeers,
  onSearchChange,
  onSelectPeer,
  onTogglePinPeer,
  onDeletePeer,
  onScanPulse,
  onOpenPair,
}) => {
  const [contextMenu, setContextMenu] = useState<{
    peer: PeerSummary;
    x: number;
    y: number;
    confirmDelete: boolean;
  } | null>(null);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", close, true);
    };
  }, [contextMenu]);

  return (
    <section
      className="chat-sidebar"
      onContextMenu={(e) => {
        if ((e.target as HTMLElement)?.tagName !== "INPUT") {
          e.preventDefault();
        }
      }}
    >
      <div className="sidebar-header">
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <div>
            <div style={{ fontSize: 16, fontWeight: 700, letterSpacing: "0.02em", color: "var(--text-main)" }}>
              {t.appName}
            </div>
            {(() => {
              const onlineCount = peers.filter((p) => isOnlineRoute(p.route)).length;
              return (
                <div
                  style={{
                    fontSize: 11,
                    color: onlineCount > 0 ? "#00FF9D" : "var(--cyan)",
                    opacity: 0.95,
                    marginTop: 1,
                    display: "flex",
                    alignItems: "center",
                    gap: 5,
                  }}
                >
                  {onlineCount > 0 && (
                    <span
                      style={{
                        width: 7,
                        height: 7,
                        borderRadius: "50%",
                        background: "#00FF9D",
                        boxShadow: "0 0 8px #00FF9D",
                        display: "inline-block",
                      }}
                    />
                  )}
                  <span>
                    {onlineCount > 0
                      ? lang === "fa"
                        ? `${onlineCount} کاربر آنلاین در شبکه`
                        : `${onlineCount} peer${onlineCount > 1 ? "s" : ""} online`
                      : peers.length > 0
                      ? lang === "fa"
                        ? `${peers.length} مخاطب (در انتظار اتصال)`
                        : `${peers.length} contact${peers.length > 1 ? "s" : ""} (connecting...)`
                      : lang === "fa"
                      ? "در حال اسکن شبکه مش..."
                      : "Scanning mesh network..."}
                  </span>
                </div>
              );
            })()}
          </div>
          <div style={{ display: "flex", gap: 4 }}>
            <button
              type="button"
              className="icon-btn"
              title={t.chatList.scanNow}
              onClick={onScanPulse}
            >
              <RefreshCw size={16} />
            </button>
            <button
              type="button"
              className="icon-btn"
              title={t.chatList.addManual}
              onClick={() => onOpenPair()}
            >
              <UserPlus size={16} />
            </button>
          </div>
        </div>

        <div className="search-box">
          <Search size={15} color="#8A9BB8" />
          <input
            placeholder={t.chatList.searchPlaceholder}
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
          />
        </div>
      </div>

      <div className="peer-list-scroll">
        {peers.length === 0 ? (
          <div
            style={{
              padding: "28px 16px",
              textAlign: "center",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 10,
              color: "var(--text-muted)",
            }}
          >
            <div
              style={{
                width: 48,
                height: 48,
                borderRadius: 16,
                background: "rgba(0, 240, 255, 0.08)",
                border: "1px solid rgba(0, 240, 255, 0.2)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: "#00F0FF",
              }}
            >
              <Globe size={22} />
            </div>
            <div style={{ fontSize: 13.5, fontWeight: 600, color: "var(--text-main)" }}>
              {t.chatList.noPeersTitle}
            </div>
            <p style={{ fontSize: 11.5, lineHeight: 1.6 }}>{t.chatList.noPeersBody}</p>
            <div style={{ display: "flex", gap: 8, marginTop: 4, flexWrap: "wrap", justifyContent: "center" }}>
              {searchQuery.trim() ? (
                <button
                  type="button"
                  className="cyber-btn"
                  onClick={() => onOpenPair(searchQuery.trim())}
                >
                  <UserPlus size={13} />
                  <span>{`${t.chatList.searchGlobalAction} ${searchQuery.trim()}`}</span>
                </button>
              ) : (
                <>
                  <button type="button" className="ghost-btn" onClick={onScanPulse}>
                    <RefreshCw size={13} />
                    <span>{t.chatList.scanNow}</span>
                  </button>
                  <button type="button" className="cyber-btn" onClick={() => onOpenPair()}>
                    <UserPlus size={13} />
                    <span>{t.chatList.addManual}</span>
                  </button>
                </>
              )}
            </div>
          </div>
        ) : (
          peers.map((peer) => {
            const active = peer.profile.peer_id === selectedPeerId;
            const isOnline = isOnlineRoute(peer.route);
            const isTyping = typingPeers[peer.profile.peer_id];

            return (
              <button
                key={peer.profile.peer_id}
                type="button"
                className={`peer-item ${active ? "active" : ""}`}
                onClick={() => onSelectPeer(peer.profile.peer_id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  const x = Math.min(e.clientX, window.innerWidth - 220);
                  const y = Math.min(e.clientY, window.innerHeight - 130);
                  setContextMenu({ peer, x: Math.max(8, x), y: Math.max(8, y), confirmDelete: false });
                }}
              >
                <CyberAvatar
                  name={peer.profile.display_name}
                  color={peer.profile.avatar_color}
                  avatarUrl={peer.profile.avatar_data}
                  size={44}
                  online={isOnline}
                  verified={peer.verified}
                />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 6,
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 5,
                        minWidth: 0,
                      }}
                    >
                      <span
                        style={{
                          fontWeight: 600,
                          fontSize: 13.5,
                          whiteSpace: "nowrap",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {peer.profile.display_name}
                      </span>
                      {peer.pinned && <Pin size={11} color="#00F0FF" />}
                    </div>

                    <span style={{ fontSize: 10.5, color: "var(--text-dim)", flexShrink: 0 }}>
                      {peer.last_message_at
                        ? formatTime(peer.last_message_at, lang)
                        : formatTime(peer.last_seen, lang)}
                    </span>
                  </div>

                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 6,
                      marginTop: 4,
                    }}
                  >
                    <span
                      style={{
                        fontSize: 12,
                        color: isTyping ? "#00FF9D" : "var(--text-muted)",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {isTyping
                        ? t.chatList.typing
                        : peer.last_message_preview ??
                          `${peer.profile.role_badge} • ${peer.profile.handle}`}
                    </span>

                    <div style={{ display: "flex", alignItems: "center", gap: 5, flexShrink: 0 }}>
                      {peer.unread_count > 0 && (
                        <span
                          style={{
                            minWidth: 19,
                            height: 19,
                            padding: "0 5px",
                            borderRadius: 999,
                            background: "#00F0FF",
                            color: "#07090F",
                            fontSize: 11,
                            fontWeight: 700,
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "center",
                          }}
                        >
                          {peer.unread_count}
                        </span>
                      )}
                      <span className={`route-pill ${routePillClass(peer.route)}`}>
                        {formatRouteLabel(peer.route, t, peer.latency_ms, true)}
                      </span>
                    </div>
                  </div>
                </div>
              </button>
            );
          })
        )}
      </div>

      {contextMenu && (
        <div
          onClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.preventDefault()}
          style={{
            position: "fixed",
            top: contextMenu.y,
            left: contextMenu.x,
            zIndex: 9999,
            minWidth: 200,
            padding: 6,
            borderRadius: 12,
            background: "rgba(13, 18, 30, 0.98)",
            border: "1px solid rgba(0, 240, 255, 0.32)",
            boxShadow: "0 14px 36px rgba(0, 0, 0, 0.65)",
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          <button
            type="button"
            className="ghost-btn"
            style={{ justifyContent: "flex-start", fontSize: 12, padding: "7px 10px" }}
            onClick={async () => {
              const target = contextMenu.peer;
              setContextMenu(null);
              await onTogglePinPeer(target.profile.peer_id, !target.pinned);
            }}
          >
            <Pin size={13} color="#00F0FF" />
            <span>{contextMenu.peer.pinned ? t.profile.unpinChat : t.profile.pinChat}</span>
          </button>

          <button
            type="button"
            className="ghost-btn"
            style={{
              justifyContent: "flex-start",
              fontSize: 12,
              padding: "7px 10px",
              color: "#FF6B8B",
              background: contextMenu.confirmDelete ? "rgba(255, 56, 100, 0.16)" : undefined,
              borderColor: contextMenu.confirmDelete ? "rgba(255, 56, 100, 0.45)" : "transparent",
            }}
            onClick={async () => {
              if (!contextMenu.confirmDelete) {
                setContextMenu({ ...contextMenu, confirmDelete: true });
                return;
              }
              const pid = contextMenu.peer.profile.peer_id;
              setContextMenu(null);
              await onDeletePeer(pid);
            }}
          >
            <Trash2 size={13} />
            <span>
              {contextMenu.confirmDelete ? t.chatList.confirmDeleteChat : t.chatList.deleteChat}
            </span>
          </button>
        </div>
      )}
    </section>
  );
};

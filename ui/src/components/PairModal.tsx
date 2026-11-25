import React, { useEffect, useState } from "react";
import QRCode from "qrcode";
import { Check, Copy, Link2, MessageSquarePlus, QrCode, Search, ShieldAlert, UserPlus, X } from "lucide-react";
import { api } from "../api/bridge";
import type { TranslationDict } from "../i18n/en";
import type { AccountSearchResult, NetworkTelemetry } from "../types/aegis";
import { CyberAvatar } from "./CyberAvatar";

interface PairModalProps {
  t: TranslationDict;
  telemetry: NetworkTelemetry | null;
  initialQuery?: string;
  onClose: () => void;
  onPeerAdded: (peerId: string) => void;
}

export const PairModal: React.FC<PairModalProps> = ({
  t,
  telemetry,
  initialQuery = "",
  onClose,
  onPeerAdded,
}) => {
  const [query, setQuery] = useState(initialQuery);
  const [results, setResults] = useState<AccountSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [qrDataUrl, setQrDataUrl] = useState<string>("");
  const [showQr, setShowQr] = useState(false);
  const [copiedId, setCopiedId] = useState(false);
  const [copiedLink, setCopiedLink] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (telemetry?.share_uri && showQr) {
      QRCode.toDataURL(telemetry.share_uri, {
        width: 176,
        margin: 1,
        color: {
          dark: "#00F0FF",
          light: "#07090F",
        },
      })
        .then(setQrDataUrl)
        .catch(() => {});
    }
  }, [telemetry?.share_uri, showQr]);

  useEffect(() => {
    void api.triggerDiscoveryPulse().catch(() => {});
  }, []);

  useEffect(() => {
    let cancelled = false;
    setSearching(true);
    const timer = setTimeout(() => {
      void api
        .searchAccounts(query.trim())
        .then((res) => {
          if (!cancelled) setResults(res);
        })
        .catch(() => {})
        .finally(() => {
          if (!cancelled) setSearching(false);
        });
    }, 180);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query]);

  const handleConnectIdentifier = async (identifier: string) => {
    const trimmed = identifier.trim();
    if (!trimmed) return;
    setError(null);
    setConnectingId(trimmed);
    try {
      const peer = await api.connectByAccount(trimmed);
      onPeerAdded(peer.profile.peer_id);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setConnectingId(null);
    }
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    void handleConnectIdentifier(query);
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-panel" style={{ maxWidth: 580 }} onClick={(e) => e.stopPropagation()}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 16,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <UserPlus size={20} color="#00F0FF" />
            <h2 style={{ fontSize: 17, fontWeight: 700 }}>{t.pair.title}</h2>
          </div>
          <button type="button" className="icon-btn" onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        {telemetry && (
          <div
            className="cyber-card"
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 10,
              marginBottom: 16,
              borderColor: "rgba(0, 240, 255, 0.25)",
              background: "rgba(0, 240, 255, 0.04)",
            }}
          >
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
              <div>
                <div style={{ fontSize: 11.5, color: "var(--text-muted)" }}>
                  {t.pair.myIdCardTitle}
                </div>
                <div className="mono selectable" style={{ fontSize: 16.5, fontWeight: 700, color: "#00F0FF", marginTop: 2 }}>
                  {telemetry.account_id}
                </div>
              </div>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                <button
                  type="button"
                  className="ghost-btn"
                  onClick={() => {
                    navigator.clipboard.writeText(telemetry.account_id);
                    setCopiedId(true);
                    setTimeout(() => setCopiedId(false), 1800);
                  }}
                >
                  {copiedId ? <Check size={14} color="#00FF9D" /> : <Copy size={14} />}
                  <span>{copiedId ? t.chat.copied : t.pair.copyId}</span>
                </button>
                <button
                  type="button"
                  className="ghost-btn"
                  onClick={() => {
                    navigator.clipboard.writeText(telemetry.share_uri);
                    setCopiedLink(true);
                    setTimeout(() => setCopiedLink(false), 1800);
                  }}
                >
                  {copiedLink ? <Check size={14} color="#00FF9D" /> : <Link2 size={14} />}
                  <span>{copiedLink ? t.chat.copied : t.pair.copyLink}</span>
                </button>
                <button
                  type="button"
                  className="icon-btn"
                  onClick={() => setShowQr((v) => !v)}
                  title="QR"
                >
                  <QrCode size={16} color={showQr ? "#00F0FF" : undefined} />
                </button>
              </div>
            </div>

            <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.5 }}>
              {t.pair.myIdHint}
            </p>

            {showQr && qrDataUrl && (
              <div style={{ display: "flex", justifyContent: "center", paddingTop: 4 }}>
                <img
                  src={qrDataUrl}
                  alt="Cymesh Account QR"
                  style={{
                    width: 156,
                    height: 156,
                    borderRadius: 14,
                    border: "1px solid rgba(0,240,255,0.3)",
                  }}
                />
              </div>
            )}
          </div>
        )}

        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <label style={{ fontSize: 12, color: "var(--text-muted)" }}>
            {t.pair.searchLabel}
          </label>
          <div style={{ display: "flex", gap: 8 }}>
            <div className="search-box" style={{ flex: 1 }}>
              <Search size={15} color="#00F0FF" />
              <input
                autoFocus
                placeholder={t.pair.searchPlaceholder}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <button
              type="submit"
              className="cyber-btn"
              disabled={connectingId !== null || !query.trim()}
            >
              <span>{t.pair.directConnectBtn}</span>
            </button>
          </div>
        </form>

        {error && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              fontSize: 12,
              color: "#FF6B8B",
              background: "rgba(255,56,100,0.12)",
              padding: "8px 10px",
              borderRadius: 10,
              marginTop: 10,
            }}
          >
            <ShieldAlert size={14} />
            <span>{error}</span>
          </div>
        )}

        <div style={{ marginTop: 14, display: "flex", flexDirection: "column", gap: 8 }}>
          <div style={{ fontSize: 11.5, fontWeight: 700, color: "#00F0FF" }}>
            {searching ? t.pair.searching : t.pair.discoverableSection}
          </div>

          {!searching && results.length === 0 ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "10px 4px" }}>
              {t.pair.noResults}
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8, maxHeight: 240, overflowY: "auto" }}>
              {results.map((item) => (
                <div
                  key={item.peer_id}
                  className="cyber-card"
                  style={{
                    padding: "10px 12px",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                  }}
                >
                  <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
                    <CyberAvatar
                      name={item.profile.display_name}
                      color={item.profile.avatar_color}
                      avatarUrl={item.profile.avatar_data}
                      size={38}
                      online={item.direct_reachable || item.relay_hops_available > 0}
                    />
                    <div style={{ minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                        <span style={{ fontWeight: 700, fontSize: 13.5 }}>
                          {item.profile.display_name}
                        </span>
                        <span className="mono" style={{ fontSize: 11.5, color: "#00F0FF" }}>
                          {item.account_id}
                        </span>
                      </div>
                      <div
                        style={{
                          fontSize: 11,
                          color: "var(--text-muted)",
                          whiteSpace: "nowrap",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                        }}
                      >
                        {item.profile.bio || item.profile.role_badge}
                      </div>
                    </div>
                  </div>

                  <button
                    type="button"
                    className={item.already_paired ? "ghost-btn" : "cyber-btn"}
                    disabled={connectingId === item.peer_id}
                    onClick={() => {
                      if (item.already_paired) {
                        onPeerAdded(item.peer_id);
                        onClose();
                      } else {
                        void handleConnectIdentifier(item.peer_id);
                      }
                    }}
                  >
                    <MessageSquarePlus size={14} />
                    <span>{item.already_paired ? t.pair.alreadyAddedBadge : t.pair.startChatBtn}</span>
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

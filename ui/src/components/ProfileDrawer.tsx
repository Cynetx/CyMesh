import React, { useEffect, useState } from "react";
import { CheckCircle2, Pin, ShieldCheck, Trash2, X, Zap } from "lucide-react";
import { api } from "../api/bridge";
import type { TranslationDict } from "../i18n/en";
import type { PeerSummary } from "../types/aegis";
import { formatRouteLabel, isDirectRoute } from "../utils/route";
import { CyberAvatar } from "./CyberAvatar";

interface ProfileDrawerProps {
  t: TranslationDict;
  peer: PeerSummary;
  onClose: () => void;
  onToggleVerify: (verified: boolean) => Promise<void>;
  onTogglePin: (pinned: boolean) => Promise<void>;
  onDeleteChat: () => Promise<void>;
}

export const ProfileDrawer: React.FC<ProfileDrawerProps> = ({
  t,
  peer,
  onClose,
  onToggleVerify,
  onTogglePin,
  onDeleteChat,
}) => {
  const [punching, setPunching] = useState(false);
  const [punchNotice, setPunchNotice] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    setConfirmDelete(false);
  }, [peer.profile.peer_id]);

  const handleUdpPunch = async () => {
    setPunching(true);
    setPunchNotice(null);
    try {
      const updated = await api.initiateUdpPunch(peer.profile.peer_id);
      setPunchNotice(
        isDirectRoute(updated.route) ? t.profile.punchSuccessToast : formatRouteLabel(updated.route, t, updated.latency_ms),
      );
    } catch (err) {
      setPunchNotice(String(err));
    } finally {
      setPunching(false);
    }
  };

  return (
    <aside className="profile-drawer">
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <span style={{ fontWeight: 700, fontSize: 14 }}>{t.profile.title}</span>
        <button type="button" className="icon-btn" onClick={onClose}>
          <X size={17} />
        </button>
      </div>

      <div
        className="cyber-card"
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          textAlign: "center",
          gap: 8,
          padding: 18,
        }}
      >
        <CyberAvatar
          name={peer.profile.display_name}
          color={peer.profile.avatar_color}
          avatarUrl={peer.profile.avatar_data}
          size={74}
          online={isDirectRoute(peer.route)}
          verified={peer.verified}
        />
        <div style={{ fontSize: 16.5, fontWeight: 700, marginTop: 2 }}>{peer.profile.display_name}</div>
        <div className="mono" style={{ fontSize: 12.5, color: "#00F0FF" }}>
          {peer.profile.handle}
        </div>
        <span
          style={{
            fontSize: 11,
            padding: "3px 12px",
            borderRadius: 999,
            background: "rgba(0, 240, 255, 0.12)",
            border: "1px solid rgba(0, 240, 255, 0.28)",
            color: "#00F0FF",
            fontWeight: 600,
          }}
        >
          {peer.profile.role_badge}
        </span>
        {peer.profile.bio && (
          <div
            style={{
              fontSize: 12.5,
              color: "var(--text-secondary)",
              lineHeight: 1.6,
              marginTop: 6,
              padding: "8px 12px",
              borderRadius: 12,
              background: "rgba(0, 0, 0, 0.25)",
              border: "1px solid var(--border-subtle)",
              width: "100%",
              wordBreak: "break-word",
            }}
          >
            {peer.profile.bio}
          </div>
        )}
      </div>

      <div className="cyber-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        {peer.account_id && (
          <div>
            <div style={{ fontSize: 11, color: "var(--text-muted)" }}>{t.profile.accountId}</div>
            <div className="mono selectable" style={{ fontSize: 13.5, fontWeight: 700, color: "#00F0FF", marginTop: 2 }}>
              {peer.account_id}
            </div>
          </div>
        )}

        <div>
          <div style={{ fontSize: 11, color: "var(--text-muted)" }}>{t.profile.fingerprint}</div>
          <div className="mono selectable" style={{ fontSize: 13, color: "#00FF9D", marginTop: 2 }}>
            {peer.fingerprint}
          </div>
        </div>

        <div>
          <div style={{ fontSize: 11, color: "var(--text-muted)" }}>{t.profile.peerId}</div>
          <div
            className="mono selectable"
            style={{ fontSize: 11, color: "var(--text-main)", wordBreak: "break-all", marginTop: 2 }}
          >
            {peer.profile.peer_id}
          </div>
        </div>

        <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12 }}>
          <span style={{ color: "var(--text-muted)" }}>{t.profile.route}</span>
          <span
            style={{
              color: isDirectRoute(peer.route)
                ? "#00FF9D"
                : peer.route === "mesh_relay"
                ? "#00F0FF"
                : "var(--text-main)",
            }}
          >
            {formatRouteLabel(peer.route, t, peer.latency_ms)}
          </span>
        </div>
      </div>

      <div className="cyber-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ fontSize: 12.5, fontWeight: 700, color: "#00F0FF" }}>
          {t.profile.safetyTitle}
        </div>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            fontSize: 22,
            padding: "8px 10px",
            borderRadius: 12,
            background: "#07090F",
          }}
        >
          {peer.safety_emojis.map((emo, idx) => (
            <span key={idx}>{emo}</span>
          ))}
        </div>
        <div
          className="mono selectable"
          style={{
            fontSize: 12,
            letterSpacing: "0.08em",
            textAlign: "center",
            color: "var(--text-main)",
            background: "#07090F",
            padding: 8,
            borderRadius: 10,
          }}
        >
          {peer.safety_number}
        </div>
        <p style={{ fontSize: 11, color: "var(--text-muted)", lineHeight: 1.55 }}>
          {t.profile.safetyHint}
        </p>

        <button
          type="button"
          className={peer.verified ? "ghost-btn" : "cyber-btn"}
          style={{
            borderColor: peer.verified ? "rgba(0,255,157,0.4)" : undefined,
            color: peer.verified ? "#00FF9D" : undefined,
          }}
          onClick={() => void onToggleVerify(!peer.verified)}
        >
          {peer.verified ? <CheckCircle2 size={15} /> : <ShieldCheck size={15} />}
          <span>{peer.verified ? t.profile.verifiedBadge : t.profile.markVerified}</span>
        </button>
      </div>

      <button
        type="button"
        className="ghost-btn"
        onClick={() => void onTogglePin(!peer.pinned)}
      >
        <Pin size={14} />
        <span>{peer.pinned ? t.profile.unpinChat : t.profile.pinChat}</span>
      </button>

      {peer.route !== "lan_direct" && (
        <button
          type="button"
          className="ghost-btn"
          disabled={punching}
          onClick={() => void handleUdpPunch()}
          style={{ borderColor: "rgba(168,85,247,0.35)", color: "#c084fc" }}
        >
          <Zap size={14} />
          <span>{t.profile.tryPunchBtn}</span>
        </button>
      )}

      {punchNotice && (
        <div style={{ fontSize: 11.5, color: "#00F0FF", textAlign: "center" }}>
          {punchNotice}
        </div>
      )}

      <button
        type="button"
        className="ghost-btn"
        onClick={async () => {
          if (!confirmDelete) {
            setConfirmDelete(true);
            return;
          }
          await onDeleteChat();
          setConfirmDelete(false);
        }}
        style={{
          marginTop: "auto",
          borderColor: "rgba(255, 56, 100, 0.35)",
          color: "#FF6B8B",
          background: confirmDelete ? "rgba(255, 56, 100, 0.14)" : undefined,
        }}
      >
        <Trash2 size={14} />
        <span>{confirmDelete ? t.profile.confirmDeleteChatBtn : t.profile.deleteChatBtn}</span>
      </button>
    </aside>
  );
};

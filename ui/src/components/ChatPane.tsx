import React, { useEffect, useRef, useState } from "react";
import {
  Check,
  CheckCheck,
  Clock,
  Copy,
  Edit3,
  Flame,
  Info,
  Radio,
  Reply,
  Send,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import { formatTime, type AppLang } from "../i18n";
import type { TranslationDict } from "../i18n/en";
import type { ChatMessage, PeerSummary } from "../types/aegis";
import { formatRouteLabel, isOnlineRoute } from "../utils/route";
import { CyberAvatar } from "./CyberAvatar";

interface ChatPaneProps {
  t: TranslationDict;
  lang: AppLang;
  myPeerId: string;
  peer: PeerSummary | null;
  messages: ChatMessage[];
  isTyping: boolean;
  showProfileDrawer: boolean;
  onToggleProfileDrawer: () => void;
  onSendMessage: (
    content: string,
    replyToId: string | null,
    expiresInSecs: number | null
  ) => Promise<void>;
  onSendTyping: (typing: boolean) => void;
  onToggleReaction: (msgId: string, emoji: string) => Promise<void>;
  onEditMessage: (msgId: string, newContent: string) => Promise<void>;
  onDeleteMessage: (msgId: string, forBoth: boolean) => Promise<void>;
}

const QUICK_REACTIONS = ["🔥", "⚡", "🛡️", "👍", "❤️"];

function formatDateSeparator(timestamp: number, lang: AppLang): string {
  const d = new Date(timestamp);
  const now = new Date();

  const isSameDay = (d1: Date, d2: Date) =>
    d1.getFullYear() === d2.getFullYear() &&
    d1.getMonth() === d2.getMonth() &&
    d1.getDate() === d2.getDate();

  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);

  if (isSameDay(d, now)) {
    return lang === "fa" ? "امروز" : "Today";
  }
  if (isSameDay(d, yesterday)) {
    return lang === "fa" ? "دیروز" : "Yesterday";
  }

  return d.toLocaleDateString(lang === "fa" ? "fa-IR" : "en-US", {
    month: "short",
    day: "numeric",
    year: d.getFullYear() !== now.getFullYear() ? "numeric" : undefined,
  });
}

export const ChatPane: React.FC<ChatPaneProps> = ({
  t,
  lang,
  myPeerId,
  peer,
  messages,
  isTyping,
  showProfileDrawer,
  onToggleProfileDrawer,
  onSendMessage,
  onSendTyping,
  onToggleReaction,
  onEditMessage,
  onDeleteMessage,
}) => {
  const [draft, setDraft] = useState("");
  const [replyTo, setReplyTo] = useState<ChatMessage | null>(null);
  const [editingMsg, setEditingMsg] = useState<ChatMessage | null>(null);
  const [ephemeralSecs, setEphemeralSecs] = useState<number | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const endRef = useRef<HTMLDivElement | null>(null);
  const typingTimerRef = useRef<number | null>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages.length, peer?.profile.peer_id]);

  useEffect(() => {
    setDraft("");
    setReplyTo(null);
    setEditingMsg(null);
  }, [peer?.profile.peer_id]);

  if (!peer) {
    return (
      <main
        className="chat-main"
        style={{ alignItems: "center", justifyContent: "center", padding: 32, textAlign: "center" }}
      >
        <div
          className="cyber-card"
          style={{
            maxWidth: 440,
            padding: 32,
            borderRadius: 24,
            borderColor: "rgba(0, 240, 255, 0.25)",
            boxShadow: "0 12px 48px rgba(0, 0, 0, 0.5), 0 0 32px rgba(0, 240, 255, 0.08)",
          }}
        >
          <div
            style={{
              width: 108,
              height: 108,
              borderRadius: 30,
              background: "radial-gradient(circle at 50% 50%, rgba(0, 240, 255, 0.25), rgba(7, 10, 18, 0.95))",
              border: "1.5px solid rgba(0, 240, 255, 0.45)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              margin: "0 auto 22px",
              boxShadow: "0 0 36px rgba(0, 240, 255, 0.28)",
              padding: 14,
            }}
          >
            <img
              src="/cymesh-cyber-logo.png"
              alt="Cymesh"
              style={{
                width: "100%",
                height: "100%",
                objectFit: "contain",
                filter: "drop-shadow(0 0 12px #00F0FF)",
              }}
            />
          </div>
          <h2 style={{ fontSize: 20, fontWeight: 700, marginBottom: 8, letterSpacing: "0.02em" }}>
            {t.chat.emptyTitle}
          </h2>
          <p style={{ fontSize: 13.5, color: "var(--text-muted)", lineHeight: 1.7 }}>
            {t.chat.emptyBody}
          </p>
        </div>
      </main>
    );
  }

  const handleDraftChange = (val: string) => {
    setDraft(val);
    onSendTyping(val.trim().length > 0);
    if (typingTimerRef.current) {
      window.clearTimeout(typingTimerRef.current);
    }
    typingTimerRef.current = window.setTimeout(() => {
      onSendTyping(false);
    }, 2200);
  };

  const handleSubmit = async () => {
    const clean = draft.trim();
    if (!clean) return;
    onSendTyping(false);
    setDraft("");

    if (editingMsg) {
      const id = editingMsg.id;
      setEditingMsg(null);
      await onEditMessage(id, clean);
      return;
    }

    const rId = replyTo?.id ?? null;
    setReplyTo(null);
    await onSendMessage(clean, rId, ephemeralSecs);
  };

  const cycleEphemeral = () => {
    const opts: Array<number | null> = [null, 30, 300, 3600];
    const idx = opts.indexOf(ephemeralSecs);
    setEphemeralSecs(opts[(idx + 1) % opts.length]);
  };

  const formatEphemeralLabel = (s: number | null) => {
    if (!s) return t.chat.ephemeralOff;
    if (s < 60) return `${t.chat.ephemeralLabel}: ${s}${t.common.seconds}`;
    if (s < 3600) return `${t.chat.ephemeralLabel}: ${Math.round(s / 60)}${t.common.minutes}`;
    return `${t.chat.ephemeralLabel}: ${Math.round(s / 3600)}${t.common.hours}`;
  };

  const renderDeliveryIcon = (msg: ChatMessage) => {
    switch (msg.status) {
      case "queued_offline":
        return <Clock size={13} color="#8A9BB8" />;
      case "relayed_mesh":
        return <Radio size={13} color="#00F0FF" />;
      case "sent_direct":
        return <Check size={13} color="#8A9BB8" />;
      case "delivered":
        return <CheckCheck size={13} color="#8A9BB8" />;
      case "read":
        return <CheckCheck size={13} color="#00F0FF" />;
      default:
        return <Clock size={13} color="#FF3864" />;
    }
  };

  const msgMap = new Map(messages.map((m) => [m.id, m]));

  return (
    <main className="chat-main">
      <header className="chat-header">
        <div
          style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0, cursor: "pointer" }}
          onClick={onToggleProfileDrawer}
        >
          <CyberAvatar
            name={peer.profile.display_name}
            color={peer.profile.avatar_color}
            avatarUrl={peer.profile.avatar_data}
            size={42}
            online={isOnlineRoute(peer.route)}
            verified={peer.verified}
          />
          <div style={{ minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
              <span style={{ fontWeight: 700, fontSize: 15 }}>
                {peer.profile.display_name}
              </span>
              <span
                style={{
                  fontSize: 10.5,
                  padding: "1px 8px",
                  borderRadius: 999,
                  background: "rgba(0, 240, 255, 0.12)",
                  color: "#00F0FF",
                  border: "1px solid rgba(0, 240, 255, 0.28)",
                  fontWeight: 600,
                }}
              >
                {peer.profile.role_badge}
              </span>
            </div>
            <div style={{ fontSize: 11.5, color: isTyping ? "#00FF9D" : "var(--text-muted)", marginTop: 2 }}>
              {isTyping
                ? t.chatList.typing
                : `${formatRouteLabel(peer.route, t, peer.latency_ms)} • ${t.chat.e2eeBadge}`}
            </div>
          </div>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <button
            type="button"
            className="ghost-btn"
            onClick={cycleEphemeral}
            style={{
              borderColor: ephemeralSecs ? "rgba(255, 184, 0, 0.45)" : undefined,
              color: ephemeralSecs ? "#FFD866" : undefined,
              background: ephemeralSecs ? "rgba(255, 184, 0, 0.1)" : undefined,
            }}
          >
            <Flame size={14} />
            <span>{formatEphemeralLabel(ephemeralSecs)}</span>
          </button>

          <button
            type="button"
            className={`icon-btn ${showProfileDrawer ? "active" : ""}`}
            onClick={onToggleProfileDrawer}
            title={t.profile.title}
          >
            <Info size={18} />
          </button>
        </div>
      </header>

      {!peer.verified && (
        <div className="verify-banner">
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <ShieldAlert size={16} />
            <span>{t.chat.verifyPrompt}</span>
          </div>
          <button
            type="button"
            className="ghost-btn"
            style={{ borderColor: "rgba(255, 184, 0, 0.45)", color: "#FFD866" }}
            onClick={() => {
              if (!showProfileDrawer) onToggleProfileDrawer();
            }}
          >
            <ShieldCheck size={14} />
            <span>{t.chat.verifyBtn}</span>
          </button>
        </div>
      )}

      <div className="messages-scroll">
        {messages.map((msg, index) => {
          const prevMsg = index > 0 ? messages[index - 1] : null;
          const prevDate = prevMsg ? new Date(prevMsg.timestamp).toDateString() : null;
          const currentDate = new Date(msg.timestamp).toDateString();
          const showDateSeparator = prevDate !== currentDate;

          const isMine = msg.sender_id === myPeerId;
          const quoted = msg.reply_to_id ? msgMap.get(msg.reply_to_id) : undefined;

          // Aggregate reactions
          const groupedReactions = msg.reactions.reduce<Record<string, { count: number; mine: boolean }>>(
            (acc, r) => {
              if (!acc[r.emoji]) acc[r.emoji] = { count: 0, mine: false };
              acc[r.emoji].count += 1;
              if (r.sender_id === myPeerId) acc[r.emoji].mine = true;
              return acc;
            },
            {}
          );

          return (
            <React.Fragment key={msg.id}>
              {showDateSeparator && (
                <div className="date-separator">
                  <span>{formatDateSeparator(msg.timestamp, lang)}</span>
                </div>
              )}

              <div className={`msg-row ${isMine ? "mine" : "theirs"}`}>
                <div className="msg-actions">
                  {QUICK_REACTIONS.map((emo) => (
                    <button
                      key={emo}
                      type="button"
                      onClick={() => void onToggleReaction(msg.id, emo)}
                      style={{
                        background: "transparent",
                        border: "none",
                        cursor: "pointer",
                        fontSize: 13,
                        padding: "2px 4px",
                        transition: "transform 0.15s ease",
                      }}
                    >
                      {emo}
                    </button>
                  ))}
                  <button
                    type="button"
                    className="icon-btn"
                    style={{ width: 26, height: 26 }}
                    title={t.chat.reply}
                    onClick={() => setReplyTo(msg)}
                  >
                    <Reply size={13} />
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    style={{ width: 26, height: 26 }}
                    title={t.chat.copy}
                    onClick={() => {
                      navigator.clipboard.writeText(msg.content);
                      setCopiedId(msg.id);
                      setTimeout(() => setCopiedId(null), 1500);
                    }}
                  >
                    {copiedId === msg.id ? <Check size={13} color="#00FF9D" /> : <Copy size={13} />}
                  </button>
                  {isMine && (
                    <button
                      type="button"
                      className="icon-btn"
                      style={{ width: 26, height: 26 }}
                      title={t.chat.edit}
                      onClick={() => {
                        setEditingMsg(msg);
                        setDraft(msg.content);
                      }}
                    >
                      <Edit3 size={13} />
                    </button>
                  )}
                  <button
                    type="button"
                    className="icon-btn"
                    style={{ width: 26, height: 26, color: "#FF6B8B" }}
                    title={t.chat.deleteBoth}
                    onClick={() => void onDeleteMessage(msg.id, true)}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>

                <div className="msg-bubble">
                  {quoted && (
                    <div className="msg-reply-quote">
                      <div style={{ fontWeight: 600, fontSize: 11, color: "#00F0FF" }}>
                        {quoted.sender_id === myPeerId
                          ? lang === "fa"
                            ? "شما"
                            : "You"
                          : peer.profile.display_name}
                      </div>
                      <div
                        style={{
                          whiteSpace: "nowrap",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          maxWidth: 280,
                        }}
                      >
                        {quoted.content}
                      </div>
                    </div>
                  )}

                  <div>{msg.content}</div>

                  {Object.keys(groupedReactions).length > 0 && (
                    <div style={{ display: "flex", gap: 5, flexWrap: "wrap", marginTop: 6 }}>
                      {Object.entries(groupedReactions).map(([emo, info]) => (
                        <button
                          key={emo}
                          type="button"
                          className={`reaction-pill ${info.mine ? "active" : ""}`}
                          onClick={() => void onToggleReaction(msg.id, emo)}
                        >
                          <span>{emo}</span>
                          <span>{info.count}</span>
                        </button>
                      ))}
                    </div>
                  )}

                  <div className="msg-meta" style={{ justifyContent: isMine ? "flex-end" : "flex-start" }}>
                    {msg.expires_at && (
                      <span style={{ color: "#FFD866", display: "inline-flex", alignItems: "center", gap: 3 }}>
                        <Flame size={11} />
                      </span>
                    )}
                    {msg.edited && <span>{t.chat.edited}</span>}
                    <span>{formatTime(msg.timestamp, lang)}</span>
                    {isMine && renderDeliveryIcon(msg)}
                  </div>
                </div>
              </div>
            </React.Fragment>
          );
        })}
        <div ref={endRef} />
      </div>

      <div className="composer-wrap">
        {(replyTo || editingMsg) && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              padding: "8px 14px",
              borderRadius: 14,
              background: "rgba(14, 20, 36, 0.92)",
              borderInlineStart: "3px solid #00F0FF",
              border: "1px solid rgba(0, 240, 255, 0.25)",
              backdropFilter: "blur(14px)",
              fontSize: 12.5,
              boxShadow: "0 4px 18px rgba(0, 0, 0, 0.35)",
            }}
          >
            <div style={{ minWidth: 0 }}>
              <span style={{ color: "#00F0FF", fontWeight: 600 }}>
                {editingMsg ? t.chat.editingMsg : `${t.chat.replyingTo}: `}
              </span>
              <span style={{ color: "var(--text-muted)", marginInlineStart: 6 }}>
                {(editingMsg ?? replyTo)?.content.slice(0, 70)}
              </span>
            </div>
            <button
              type="button"
              className="icon-btn"
              style={{ width: 24, height: 24 }}
              onClick={() => {
                setReplyTo(null);
                setEditingMsg(null);
                if (editingMsg) setDraft("");
              }}
            >
              <X size={14} />
            </button>
          </div>
        )}

        <div className="composer-box">
          <textarea
            rows={1}
            placeholder={t.chat.inputPlaceholder}
            value={draft}
            onChange={(e) => handleDraftChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void handleSubmit();
              }
            }}
          />
          <button
            type="button"
            className="cyber-btn"
            style={{
              padding: "8px 16px",
              borderRadius: 999,
              background: draft.trim()
                ? "linear-gradient(135deg, rgba(0, 240, 255, 0.35) 0%, rgba(0, 255, 157, 0.22) 100%)"
                : undefined,
              boxShadow: draft.trim()
                ? "0 0 16px rgba(0, 240, 255, 0.3)"
                : undefined,
            }}
            disabled={!draft.trim()}
            onClick={() => void handleSubmit()}
          >
            <Send size={15} />
            <span>{editingMsg ? t.chat.saveEdit : t.chat.send}</span>
          </button>
        </div>
      </div>
    </main>
  );
};

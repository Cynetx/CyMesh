import React, { useState } from "react";
import { Eye, EyeOff, Globe, KeyRound, RotateCcw, ShieldAlert, Unlock } from "lucide-react";
import type { AppLang } from "../i18n";
import type { TranslationDict } from "../i18n/en";
import { normalizePin } from "../utils/pin";

interface LockScreenProps {
  t: TranslationDict;
  lang: AppLang;
  onToggleLang: () => void;
  onUnlock: (pin: string) => Promise<void>;
  onResetVault: (openRecover: boolean) => Promise<void>;
}

export const LockScreen: React.FC<LockScreenProps> = ({
  t,
  lang,
  onToggleLang,
  onUnlock,
  onResetVault,
}) => {
  const [pin, setPin] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [capsLock, setCapsLock] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [showResetConfirm, setShowResetConfirm] = useState(false);

  const handleKeyModifier = (e: React.KeyboardEvent) => {
    setCapsLock(e.getModifierState("CapsLock"));
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const cleanPin = normalizePin(pin);
    if (!cleanPin || loading) return;
    setLoading(true);
    setError(null);
    try {
      await onUnlock(cleanPin);
    } catch (err: unknown) {
      const msg = String(err || "");
      if (msg.includes("InvalidCredentials") || msg.includes("credentials") || msg.includes("password")) {
        setError(t.lock.wrongPin);
      } else {
        setError(msg || t.lock.wrongPin);
      }
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={{
        height: "100vh",
        width: "100vw",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background:
          "radial-gradient(circle at 50% 20%, rgba(0,240,255,0.12), transparent 50%), #07090F",
        padding: 20,
      }}
    >
      <div
        className="cyber-card"
        style={{
          width: "100%",
          maxWidth: 420,
          padding: 28,
          borderRadius: 22,
          borderColor: "rgba(0, 240, 255, 0.25)",
          boxShadow: "0 20px 60px rgba(0,0,0,0.65)",
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 22,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
            <img src="/cymesh-cyber-logo.png" alt="Cymesh" style={{ width: 17, height: 17, objectFit: "contain" }} />
            <span
              style={{
                fontSize: 12,
                fontWeight: 700,
                color: "#00F0FF",
                letterSpacing: "0.06em",
              }}
            >
              {t.appName.toUpperCase()}
            </span>
          </div>
          <button type="button" className="ghost-btn" onClick={onToggleLang}>
            <Globe size={14} />
            <span>{lang === "fa" ? "English" : "فارسی"}</span>
          </button>
        </div>

        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            textAlign: "center",
            gap: 12,
            marginBottom: 22,
          }}
        >
          <div
            style={{
              width: 96,
              height: 96,
              borderRadius: 26,
              background: "radial-gradient(circle at 50% 50%, rgba(0, 240, 255, 0.25), rgba(7, 10, 18, 0.95))",
              border: "1.5px solid rgba(0, 240, 255, 0.5)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              boxShadow: "0 0 38px rgba(0, 240, 255, 0.3)",
              padding: 12,
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
          <h2 style={{ fontSize: 20, fontWeight: 700 }}>{t.lock.title}</h2>
          <p style={{ fontSize: 12, color: "var(--text-muted)", lineHeight: 1.6 }}>
            {t.lock.subtitle}
          </p>
        </div>

        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
            <input
              type={showPassword ? "text" : "password"}
              className="cyber-input"
              placeholder={t.lock.pinPlaceholder}
              value={pin}
              onChange={(e) => setPin(e.target.value)}
              onKeyDown={handleKeyModifier}
              onKeyUp={handleKeyModifier}
              autoFocus
              style={{
                width: "100%",
                textAlign: "center",
                fontSize: 16,
                padding: "11px 40px 11px 14px",
                letterSpacing: showPassword ? "0.08em" : "0.22em",
              }}
            />
            <button
              type="button"
              className="icon-btn"
              onClick={() => setShowPassword(!showPassword)}
              style={{
                position: "absolute",
                right: 10,
                background: "transparent",
                border: "none",
                cursor: "pointer",
                color: showPassword ? "#00F0FF" : "var(--text-muted)",
              }}
              title={showPassword ? "مخفی کردن" : "نمایش رمز"}
            >
              {showPassword ? <EyeOff size={16} /> : <Eye size={16} />}
            </button>
          </div>

          {capsLock && (
            <div style={{ fontSize: 11, color: "#FFD866", textAlign: "center" }}>
              ⚠️ {t.lock.capsLockWarning}
            </div>
          )}

          {error && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                fontSize: 12,
                color: "#FF6B8B",
                background: "rgba(255, 56, 100, 0.1)",
                border: "1px solid rgba(255, 56, 100, 0.3)",
                padding: "8px 10px",
                borderRadius: 10,
              }}
            >
              <ShieldAlert size={14} />
              <span>{error}</span>
            </div>
          )}
          <button
            type="submit"
            className="cyber-btn"
            disabled={loading || !pin.trim()}
            style={{ width: "100%", padding: "11px 16px" }}
          >
            <Unlock size={16} />
            <span>{t.lock.unlockBtn}</span>
          </button>
        </form>

        <div style={{ marginTop: 14, textAlign: "center" }}>
          {!showResetConfirm ? (
            <button
              type="button"
              className="ghost-btn"
              onClick={() => setShowResetConfirm(true)}
              style={{ width: "100%", fontSize: 11.5, color: "var(--text-muted)" }}
            >
              <RotateCcw size={13} />
              <span>{t.lock.forgotPinBtn}</span>
            </button>
          ) : (
            <div
              style={{
                marginTop: 8,
                padding: 12,
                borderRadius: 12,
                background: "rgba(255, 56, 100, 0.08)",
                border: "1px solid rgba(255, 56, 100, 0.28)",
                textAlign: "start",
                display: "flex",
                flexDirection: "column",
                gap: 8,
              }}
            >
              <div style={{ fontSize: 12.5, fontWeight: 700, color: "#FF6B8B" }}>
                {t.lock.resetWarnTitle}
              </div>
              <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.55, margin: 0 }}>
                {t.lock.resetWarnBody}
              </p>
              <button
                type="button"
                className="cyber-btn"
                disabled={loading}
                onClick={async () => {
                  setLoading(true);
                  try {
                    await onResetVault(true);
                  } finally {
                    setLoading(false);
                  }
                }}
                style={{ width: "100%", padding: "9px 12px", fontSize: 12 }}
              >
                <KeyRound size={14} />
                <span>{t.lock.resetRecoverBtn}</span>
              </button>
              <div style={{ display: "flex", gap: 8 }}>
                <button
                  type="button"
                  className="ghost-btn"
                  disabled={loading}
                  onClick={async () => {
                    setLoading(true);
                    try {
                      await onResetVault(false);
                    } finally {
                      setLoading(false);
                    }
                  }}
                  style={{ flex: 1, fontSize: 11.5, color: "#FF6B8B", borderColor: "rgba(255,56,100,0.3)" }}
                >
                  {t.lock.resetNewBtn}
                </button>
                <button
                  type="button"
                  className="ghost-btn"
                  disabled={loading}
                  onClick={() => setShowResetConfirm(false)}
                  style={{ fontSize: 11.5 }}
                >
                  {t.lock.resetCancelBtn}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

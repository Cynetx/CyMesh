import React, { useState } from "react";
import { Check, ChevronDown, ChevronUp, Copy, Eye, EyeOff, Globe, KeyRound, Shield, ShieldAlert, Sparkles } from "lucide-react";
import { api } from "../api/bridge";
import type { AppLang } from "../i18n";
import type { TranslationDict } from "../i18n/en";
import type { DuressAction, IdentityCreatedBundle } from "../types/aegis";
import { normalizePin } from "../utils/pin";

interface OnboardingModalProps {
  t: TranslationDict;
  lang: AppLang;
  initialMode?: "create" | "recover";
  onToggleLang: () => void;
  onComplete: () => Promise<void>;
}

const AVATAR_COLORS = ["#00F0FF", "#00FF9D", "#7C5CFF", "#FF4D8D", "#FFB800"];

export const OnboardingModal: React.FC<OnboardingModalProps> = ({
  t,
  lang,
  initialMode = "create",
  onToggleLang,
  onComplete,
}) => {
  const [mode, setMode] = useState<"create" | "recover">(initialMode);
  const [displayName, setDisplayName] = useState("");
  const [handle, setHandle] = useState("@ghost");
  const [roleBadge, setRoleBadge] = useState("Cipher Node");
  const [bio, setBio] = useState("");
  const [avatarColor, setAvatarColor] = useState(AVATAR_COLORS[0]);
  const [pin, setPin] = useState("");
  const [showPin, setShowPin] = useState(false);
  const [useDuress, setUseDuress] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [duressPin, setDuressPin] = useState("");
  const [showDuressPin, setShowDuressPin] = useState(false);
  const [duressAction, setDuressAction] = useState<DuressAction>("decoy");
  const [mnemonic, setMnemonic] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [createdBundle, setCreatedBundle] = useState<IdentityCreatedBundle | null>(null);
  const [copiedSeed, setCopiedSeed] = useState(false);
  const [copiedAccountId, setCopiedAccountId] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    const cleanPin = normalizePin(pin);
    if (cleanPin.length < 4) {
      setError(t.onboarding.pinPlaceholder);
      return;
    }
    if (!displayName.trim()) {
      setError(t.onboarding.displayNamePlaceholder);
      return;
    }
    setLoading(true);
    try {
      const cleanDuress = useDuress && duressPin.trim().length >= 4 ? normalizePin(duressPin) : null;
      if (mode === "create") {
        const bundle = await api.createIdentityAndVault({
          pin: cleanPin,
          displayName: displayName.trim(),
          handle: handle.trim() || "@ghost",
          bio: bio.trim(),
          roleBadge: roleBadge.trim() || "Mesh Node",
          avatarColor,
          duressPin: cleanDuress,
          duressAction: useDuress ? duressAction : null,
        });
        // Persist chosen UI language immediately
        const currentSettings = await api.getSettings();
        await api.updateSettings({ ...currentSettings, language: lang });
        setCreatedBundle(bundle);
      } else {
        await api.recoverIdentityAndVault({
          mnemonic: mnemonic.trim(),
          pin: cleanPin,
          displayName: displayName.trim(),
          handle: handle.trim() || "@ghost",
          bio: bio.trim(),
          roleBadge: roleBadge.trim() || "Mesh Node",
          avatarColor,
        });
        const currentSettings = await api.getSettings();
        await api.updateSettings({ ...currentSettings, language: lang });
        await onComplete();
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  if (createdBundle) {
    return (
      <div
        style={{
          height: "100vh",
          width: "100vw",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background:
            "radial-gradient(circle at 50% 20%, rgba(0,255,157,0.12), transparent 55%), #07090F",
          padding: 20,
        }}
      >
        <div className="modal-panel" style={{ maxWidth: 520 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12 }}>
            <KeyRound size={22} color="#00FF9D" />
            <h2 style={{ fontSize: 19, fontWeight: 700 }}>{t.onboarding.seedModalTitle}</h2>
          </div>

          <div
            className="cyber-card"
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 10,
              marginBottom: 14,
              borderColor: "rgba(0, 240, 255, 0.35)",
              background: "rgba(0, 240, 255, 0.06)",
            }}
          >
            <div>
              <div style={{ fontSize: 11, color: "var(--text-muted)" }}>
                {t.onboarding.seedModalAccountLabel}
              </div>
              <div className="mono selectable" style={{ fontSize: 16, fontWeight: 700, color: "#00F0FF", marginTop: 2 }}>
                {createdBundle.account_id}
              </div>
            </div>
            <button
              type="button"
              className="ghost-btn"
              onClick={() => {
                navigator.clipboard.writeText(createdBundle.account_id);
                setCopiedAccountId(true);
                setTimeout(() => setCopiedAccountId(false), 1800);
              }}
            >
              {copiedAccountId ? <Check size={14} color="#00FF9D" /> : <Copy size={14} />}
              <span>{copiedAccountId ? t.onboarding.seedCopied : t.pair.copyId}</span>
            </button>
          </div>

          <p style={{ fontSize: 13, color: "#FFD866", lineHeight: 1.6, marginBottom: 12 }}>
            {t.onboarding.seedModalWarn}
          </p>
          <div
            className="cyber-card mono selectable"
            style={{
              background: "#07090F",
              borderColor: "rgba(0, 255, 157, 0.35)",
              padding: 16,
              fontSize: 14,
              lineHeight: 1.8,
              color: "#00FF9D",
              marginBottom: 16,
              textAlign: "center",
            }}
          >
            {createdBundle.recovery_mnemonic}
          </div>
          <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
            <button
              type="button"
              className="ghost-btn"
              onClick={() => {
                navigator.clipboard.writeText(createdBundle.recovery_mnemonic);
                setCopiedSeed(true);
                setTimeout(() => setCopiedSeed(false), 2000);
              }}
            >
              {copiedSeed ? <Check size={15} color="#00FF9D" /> : <Copy size={15} />}
              <span>{copiedSeed ? t.onboarding.seedCopied : t.onboarding.copySeed}</span>
            </button>
            <button
              type="button"
              className="cyber-btn"
              style={{ flex: 1 }}
              onClick={() => void onComplete()}
            >
              <Sparkles size={16} />
              <span>{t.onboarding.continueApp}</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      style={{
        height: "100vh",
        width: "100vw",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background:
          "radial-gradient(circle at 30% 15%, rgba(0,240,255,0.12), transparent 48%), radial-gradient(circle at 80% 85%, rgba(0,255,157,0.08), transparent 45%), #07090F",
        padding: 20,
        overflowY: "auto",
      }}
    >
      <div className="modal-panel" style={{ maxWidth: 580 }}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 14,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 11 }}>
            <div
              style={{
                width: 42,
                height: 42,
                borderRadius: 14,
                background: "radial-gradient(circle at 50% 50%, rgba(0, 240, 255, 0.2), rgba(10, 18, 32, 0.9))",
                border: "1px solid rgba(0, 240, 255, 0.4)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                boxShadow: "0 0 16px rgba(0, 240, 255, 0.25)",
                padding: 6,
              }}
            >
              <img
                src="/cymesh-cyber-logo.png"
                alt="Cymesh"
                style={{
                  width: "100%",
                  height: "100%",
                  objectFit: "contain",
                  filter: "drop-shadow(0 0 6px rgba(0, 240, 255, 0.6))",
                }}
              />
            </div>
            <div>
              <h1 style={{ fontSize: 18, fontWeight: 700 }}>
                {mode === "create" ? t.onboarding.titleCreate : t.onboarding.titleRecover}
              </h1>
              <p style={{ fontSize: 11.5, color: "var(--text-muted)" }}>{t.onboarding.subtitle}</p>
            </div>
          </div>

          <button type="button" className="ghost-btn" onClick={onToggleLang}>
            <Globe size={14} />
            <span>{lang === "fa" ? "English" : "فارسی"}</span>
          </button>
        </div>

        <div style={{ display: "flex", gap: 8, marginBottom: 16 }}>
          <button
            type="button"
            className={mode === "create" ? "cyber-btn" : "ghost-btn"}
            style={{ flex: 1 }}
            onClick={() => setMode("create")}
          >
            {t.onboarding.tabCreate}
          </button>
          <button
            type="button"
            className={mode === "recover" ? "cyber-btn" : "ghost-btn"}
            style={{ flex: 1 }}
            onClick={() => setMode("recover")}
          >
            {t.onboarding.tabRecover}
          </button>
        </div>

        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {mode === "recover" && (
            <div>
              <label style={{ fontSize: 12, color: "var(--text-muted)", display: "block", marginBottom: 5 }}>
                {t.onboarding.mnemonicLabel}
              </label>
              <textarea
                className="cyber-input mono"
                rows={2}
                placeholder={t.onboarding.mnemonicPlaceholder}
                value={mnemonic}
                onChange={(e) => setMnemonic(e.target.value)}
              />
            </div>
          )}

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
            <div>
              <label style={{ fontSize: 12, color: "var(--text-muted)", display: "block", marginBottom: 5 }}>
                {t.onboarding.displayName}
              </label>
              <input
                className="cyber-input"
                placeholder={t.onboarding.displayNamePlaceholder}
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
              />
            </div>
            <div>
              <label style={{ fontSize: 12, color: "var(--text-muted)", display: "block", marginBottom: 5 }}>
                {t.onboarding.handle}
              </label>
              <input
                className="cyber-input mono"
                placeholder={t.onboarding.handlePlaceholder}
                value={handle}
                onChange={(e) => setHandle(e.target.value)}
              />
            </div>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr auto", gap: 12, alignItems: "center" }}>
            <div>
              <label style={{ fontSize: 12, color: "var(--text-muted)", display: "block", marginBottom: 5 }}>
                {t.onboarding.pinLabel}
              </label>
              <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
                <input
                  type={showPin ? "text" : "password"}
                  className="cyber-input"
                  placeholder={t.onboarding.pinPlaceholder}
                  value={pin}
                  onChange={(e) => setPin(e.target.value)}
                  style={{
                    paddingRight: 38,
                    letterSpacing: showPin ? "0.06em" : "0.18em",
                  }}
                />
                <button
                  type="button"
                  className="icon-btn"
                  onClick={() => setShowPin(!showPin)}
                  style={{
                    position: "absolute",
                    right: 8,
                    background: "transparent",
                    border: "none",
                    cursor: "pointer",
                    color: showPin ? "#00F0FF" : "var(--text-muted)",
                  }}
                  title={showPin ? "مخفی کردن" : "نمایش رمز"}
                >
                  {showPin ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
            </div>
            <div>
              <label style={{ fontSize: 12, color: "var(--text-muted)", display: "block", marginBottom: 5 }}>
                {t.onboarding.avatarColor}
              </label>
              <div style={{ display: "flex", gap: 7, paddingTop: 4 }}>
                {AVATAR_COLORS.map((c) => (
                  <button
                    key={c}
                    type="button"
                    onClick={() => setAvatarColor(c)}
                    style={{
                      width: 24,
                      height: 24,
                      borderRadius: 8,
                      background: c,
                      border: avatarColor === c ? "2px solid #fff" : "2px solid transparent",
                      boxShadow: avatarColor === c ? `0 0 12px ${c}` : "none",
                      cursor: "pointer",
                    }}
                  />
                ))}
              </div>
            </div>
          </div>

          <button
            type="button"
            className="ghost-btn"
            style={{ justifyContent: "space-between" }}
            onClick={() => setShowAdvanced((v) => !v)}
          >
            <span>{t.onboarding.advancedToggle}</span>
            {showAdvanced ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
          </button>

          {showAdvanced && (
            <div className="cyber-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <div>
                <label style={{ fontSize: 12, color: "var(--text-muted)", display: "block", marginBottom: 5 }}>
                  {t.onboarding.roleBadge}
                </label>
                <input
                  className="cyber-input"
                  placeholder={t.onboarding.rolePlaceholder}
                  value={roleBadge}
                  onChange={(e) => setRoleBadge(e.target.value)}
                />
              </div>
              <div>
                <label style={{ fontSize: 12, color: "var(--text-muted)", display: "block", marginBottom: 5 }}>
                  {t.onboarding.bio}
                </label>
                <input
                  className="cyber-input"
                  placeholder={t.onboarding.bioPlaceholder}
                  value={bio}
                  onChange={(e) => setBio(e.target.value)}
                />
              </div>

              {mode === "create" && (
                <div style={{ padding: 10, borderRadius: 12, background: "rgba(255,184,0,0.06)", border: "1px solid rgba(255,184,0,0.2)" }}>
                  <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer", fontSize: 12.5 }}>
                    <input
                      type="checkbox"
                      checked={useDuress}
                      onChange={(e) => setUseDuress(e.target.checked)}
                    />
                    <span style={{ fontWeight: 600, color: "#FFD866" }}>{t.onboarding.duressToggle}</span>
                  </label>
                  <p style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 4 }}>
                    {t.onboarding.duressHint}
                  </p>
                  {useDuress && (
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8, marginTop: 10 }}>
                      <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
                        <input
                          type={showDuressPin ? "text" : "password"}
                          className="cyber-input"
                          placeholder={t.onboarding.duressPinLabel}
                          value={duressPin}
                          onChange={(e) => setDuressPin(e.target.value)}
                          style={{
                            width: "100%",
                            paddingRight: 36,
                            letterSpacing: showDuressPin ? "0.06em" : "0.18em",
                          }}
                        />
                        <button
                          type="button"
                          className="icon-btn"
                          onClick={() => setShowDuressPin(!showDuressPin)}
                          style={{
                            position: "absolute",
                            right: 8,
                            background: "transparent",
                            border: "none",
                            cursor: "pointer",
                            color: showDuressPin ? "#00F0FF" : "var(--text-muted)",
                          }}
                        >
                          {showDuressPin ? <EyeOff size={15} /> : <Eye size={15} />}
                        </button>
                      </div>
                      <select
                        className="cyber-input"
                        value={duressAction}
                        onChange={(e) => setDuressAction(e.target.value as DuressAction)}
                      >
                        <option value="decoy">{t.onboarding.duressModeDecoy}</option>
                        <option value="wipe">{t.onboarding.duressModeWipe}</option>
                      </select>
                    </div>
                  )}
                </div>
              )}
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
                background: "rgba(255,56,100,0.12)",
                padding: "8px 10px",
                borderRadius: 10,
              }}
            >
              <ShieldAlert size={14} />
              <span>{error}</span>
            </div>
          )}

          <button type="submit" className="cyber-btn" disabled={loading} style={{ padding: "11px 16px" }}>
            <Shield size={16} />
            <span>{mode === "create" ? t.onboarding.btnCreate : t.onboarding.btnRecover}</span>
          </button>
        </form>
      </div>
    </div>
  );
};

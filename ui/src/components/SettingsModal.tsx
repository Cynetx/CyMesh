import React, { useRef, useState } from "react";
import { Activity, AlertTriangle, Camera, Check, ChevronDown, ChevronUp, Copy, Laptop, Plus, RotateCcw, Server, ShieldCheck, Sprout, Terminal, Trash2, X } from "lucide-react";
import { api } from "../api/bridge";
import type { TranslationDict } from "../i18n/en";
import { CyberAvatar } from "./CyberAvatar";
import { processAndSanitizeAvatar } from "../utils/avatarSecurity";
import type {
  AppSettings,
  DuressAction,
  NetworkTelemetry,
  ServerHealthResult,
  SignedProfileCard,
} from "../types/aegis";

const VOLUNTEER_RELAY_CMD = String.raw`cargo build --locked --release -p aegis-core --bin cymesh-relay &&
read -r -s -p 'Relay vault passphrase: ' relay_pin &&
printf '\n' &&
CYMESH_RELAY_PIN="$relay_pin" ./target/release/cymesh-relay
unset relay_pin`;

interface SettingsModalProps {
  t: TranslationDict;
  platform: string;
  profile: SignedProfileCard;
  settings: AppSettings;
  telemetry: NetworkTelemetry | null;
  onClose: () => void;
  onProfileUpdated: (p: SignedProfileCard) => void;
  onSettingsUpdated: (s: AppSettings) => void;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({
  t,
  platform,
  profile,
  settings,
  telemetry,
  onClose,
  onProfileUpdated,
  onSettingsUpdated,
}) => {
  const [displayName, setDisplayName] = useState(profile.display_name);
  const [handle, setHandle] = useState(profile.handle);
  const [roleBadge, setRoleBadge] = useState(profile.role_badge);
  const [bio, setBio] = useState(profile.bio);
  const [avatarColor, setAvatarColor] = useState(profile.avatar_color);
  const [avatarData, setAvatarData] = useState<string | null>(profile.avatar_data ?? null);
  const [avatarProcessing, setAvatarProcessing] = useState(false);
  const [avatarError, setAvatarError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const [stealth, setStealth] = useState(settings.stealth_mode);
  const [meshRelay, setMeshRelay] = useState(settings.mesh_relay_enabled);
  const [dhtEnabled, setDhtEnabled] = useState(settings.dht_enabled ?? true);
  const [holePunchEnabled, setHolePunchEnabled] = useState(settings.hole_punch_enabled ?? true);
  const [onionRouting, setOnionRouting] = useState(settings.onion_routing_enabled ?? false);
  const [enforceIranIp, setEnforceIranIp] = useState(settings.enforce_iran_ip ?? false);
  const [bootstrapPeers, setBootstrapPeers] = useState<string[]>(settings.bootstrap_peers ?? []);
  const [newSeedAddr, setNewSeedAddr] = useState("");
  const [seedBundleInput, setSeedBundleInput] = useState("");
  const [showAdvancedBridge, setShowAdvancedBridge] = useState(false);

  const [duressPin, setDuressPin] = useState("");
  const [duressAction, setDuressAction] = useState<DuressAction>("decoy");
  const [volunteerAddr, setVolunteerAddr] = useState("");
  const [volunteerBusy, setVolunteerBusy] = useState(false);
  const [volunteerStatus, setVolunteerStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [copiedRelayCmd, setCopiedRelayCmd] = useState(false);
  const [serverHealthTarget, setServerHealthTarget] = useState("");
  const [serverHealthBusy, setServerHealthBusy] = useState(false);
  const [serverHealthResult, setServerHealthResult] = useState<ServerHealthResult | null>(null);
  const [showResetConfirm, setShowResetConfirm] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

  const handleFactoryReset = async () => {
    setResetting(true);
    try {
      await api.resetVault();
      window.location.reload();
    } catch (err) {
      console.error("Failed to reset vault:", err);
      setResetting(false);
    }
  };

  const showToast = (msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2600);
  };

  const handleAvatarFileSelected = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setAvatarError(null);
    setAvatarProcessing(true);
    try {
      const res = await processAndSanitizeAvatar(file);
      setAvatarData(res.dataUri);
      showToast(t.settings.avatarPrivacyNotice);
    } catch (err: any) {
      setAvatarError(err?.message || "Error processing image");
    } finally {
      setAvatarProcessing(false);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  };

  const handleRemoveAvatar = () => {
    setAvatarData(null);
    setAvatarError(null);
    showToast(t.settings.removeAvatar);
  };

  const handleSaveProfile = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      const updated = await api.updateOwnProfile({
        displayName: displayName.trim(),
        handle: handle.trim(),
        bio: bio.trim(),
        roleBadge: roleBadge.trim(),
        avatarColor,
        avatarData: avatarData ? avatarData.trim() : null,
        status: stealth ? "stealth" : "online",
      });
      onProfileUpdated(updated);
      showToast(t.settings.savedToast);
    } catch (err: any) {
      showToast(String(err));
    }
  };

  const handleSaveSettings = async () => {
    const next: AppSettings = {
      ...settings,
      stealth_mode: stealth,
      mesh_relay_enabled: meshRelay,
      dht_enabled: dhtEnabled,
      hole_punch_enabled: holePunchEnabled,
      onion_routing_enabled: onionRouting,
      enforce_iran_ip: enforceIranIp,
      bootstrap_peers: bootstrapPeers,
    };
    await api.updateSettings(next);
    onSettingsUpdated(next);
    showToast(t.settings.savedToast);
  };

  const handleAddBootstrap = async () => {
    const target = newSeedAddr.trim();
    if (!target) return;
    try {
      const updated = await api.addBootstrapPeer(target);
      setBootstrapPeers(updated);
      onSettingsUpdated({ ...settings, bootstrap_peers: updated });
      setNewSeedAddr("");
      showToast(t.settings.savedToast);
    } catch (err) {
      showToast(String(err));
    }
  };

  const handleRemoveBootstrap = async (target: string) => {
    try {
      const updated = await api.removeBootstrapPeer(target);
      setBootstrapPeers(updated);
      onSettingsUpdated({ ...settings, bootstrap_peers: updated });
    } catch (err) {
      showToast(String(err));
    }
  };

  const handleExportSeeds = async () => {
    try {
      const bundle = await api.exportSeedBundle();
      await navigator.clipboard.writeText(bundle);
      showToast(t.chat.copied);
    } catch (err) {
      showToast(String(err));
    }
  };

  const handleImportSeeds = async () => {
    const bundle = seedBundleInput.trim();
    if (!bundle) return;
    try {
      const count = await api.importSeedBundle(bundle);
      setSeedBundleInput("");
      showToast(`${t.settings.seedsImportedToast} (${count})`);
    } catch (err) {
      showToast(String(err));
    }
  };

  const handleSetDuress = async () => {
    if (duressPin.trim().length < 4) return;
    await api.configureDuressPin(duressPin, duressAction);
    setDuressPin("");
    showToast(t.settings.savedToast);
  };

  const handleInstallWindows = async () => {
    try {
      const path = await api.installWindowsShortcuts();
      showToast(`${t.settings.windowsSuccess} ${path}`);
    } catch (err) {
      showToast(String(err));
    }
  };

  const handleCopyRelayCmd = async () => {
    await navigator.clipboard.writeText(VOLUNTEER_RELAY_CMD);
    setCopiedRelayCmd(true);
    setTimeout(() => setCopiedRelayCmd(false), 2000);
  };

  const handleRegisterVolunteerRelay = async () => {
    const target = volunteerAddr.trim();
    if (!target || volunteerBusy) return;
    setVolunteerBusy(true);
    setVolunteerStatus(null);
    try {
      const res = await api.registerVolunteerRelay(target);
      const updated = Array.from(new Set([...bootstrapPeers, res.addr]));
      setBootstrapPeers(updated);
      onSettingsUpdated({ ...settings, bootstrap_peers: updated });
      setVolunteerAddr("");
      setVolunteerStatus({
        ok: true,
        text: `${t.settings.volunteerSuccess}: ${res.addr} (${res.latency_ms}ms)`,
      });
      showToast(`${t.settings.volunteerSuccess} (${res.latency_ms}ms)`);
    } catch (err) {
      setVolunteerStatus({ ok: false, text: String(err) });
    } finally {
      setVolunteerBusy(false);
    }
  };

  const handleTestServerHealth = async () => {
    const target = serverHealthTarget.trim();
    if (!target || serverHealthBusy) return;
    setServerHealthBusy(true);
    try {
      const res = await api.testServerHealth(target);
      setServerHealthResult(res);
      if (res.reachable) {
        showToast(`${t.settings.serverHealthOnline} (${res.latency_ms ?? 0}ms)`);
      } else {
        showToast(t.settings.serverHealthOffline);
      }
    } catch (err) {
      setServerHealthResult({
        target,
        reachable: false,
        noise_handshake_ok: false,
        message: String(err),
        timestamp: Date.now(),
      });
      showToast(String(err));
    } finally {
      setServerHealthBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-panel" style={{ maxWidth: 620 }} onClick={(e) => e.stopPropagation()}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 16,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <div
              style={{
                width: 32,
                height: 32,
                borderRadius: 9,
                background: "rgba(0, 240, 255, 0.12)",
                border: "1px solid rgba(0, 240, 255, 0.35)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                padding: 4,
              }}
            >
              <img
                src="/cymesh-cyber-logo.png"
                alt="Cymesh"
                style={{ width: "100%", height: "100%", objectFit: "contain", filter: "drop-shadow(0 0 5px #00F0FF)" }}
              />
            </div>
            <h2 style={{ fontSize: 17, fontWeight: 700 }}>{t.settings.title}</h2>
          </div>
          <button type="button" className="icon-btn" onClick={onClose}>
            <X size={18} />
          </button>
        </div>

        {toast && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "9px 12px",
              borderRadius: 12,
              background: "rgba(0,255,157,0.12)",
              border: "1px solid rgba(0,255,157,0.35)",
              color: "#00FF9D",
              fontSize: 12.5,
              marginBottom: 14,
            }}
          >
            <Check size={15} />
            <span>{toast}</span>
          </div>
        )}

        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <form onSubmit={handleSaveProfile} className="cyber-card" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <div style={{ fontWeight: 700, fontSize: 14, color: "#00F0FF", display: "flex", alignItems: "center", gap: 6 }}>
                <ShieldCheck size={16} />
                <span>{t.settings.profileSection}</span>
              </div>
              {telemetry?.account_id && (
                <button
                  type="button"
                  className="ghost-btn mono"
                  style={{ fontSize: 11.5, color: "#00FF9D" }}
                  onClick={() => {
                    navigator.clipboard.writeText(telemetry.account_id);
                    showToast(t.chat.copied);
                  }}
                >
                  <Copy size={12} />
                  <span>{telemetry.account_id}</span>
                </button>
              )}
            </div>

            {/* Avatar & Photo Management */}
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 16,
                padding: "14px",
                borderRadius: 16,
                background: "rgba(0, 0, 0, 0.28)",
                border: "1px solid var(--border-subtle)",
              }}
            >
              <div style={{ position: "relative" }}>
                <CyberAvatar
                  name={displayName || "A"}
                  color={avatarColor}
                  avatarUrl={avatarData}
                  size={74}
                  online={!stealth}
                />
                {avatarProcessing && (
                  <div
                    style={{
                      position: "absolute",
                      inset: 0,
                      borderRadius: Math.round(74 * 0.34),
                      background: "rgba(0,0,0,0.65)",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      fontSize: 10,
                      color: "#00F0FF",
                    }}
                  >
                    ...
                  </div>
                )}
              </div>

              <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6 }}>
                <div style={{ fontSize: 13, fontWeight: 600 }}>{t.settings.avatarSection}</div>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  <button
                    type="button"
                    className="cyber-btn"
                    style={{ padding: "6px 12px", fontSize: 12 }}
                    onClick={() => fileInputRef.current?.click()}
                    disabled={avatarProcessing}
                  >
                    <Camera size={13} />
                    <span>{avatarData ? t.settings.changeAvatar : t.settings.uploadAvatar}</span>
                  </button>

                  {avatarData && (
                    <button
                      type="button"
                      className="ghost-btn"
                      style={{ padding: "6px 10px", fontSize: 12, color: "#FF6B8B", borderColor: "rgba(255, 56, 100, 0.3)" }}
                      onClick={handleRemoveAvatar}
                      disabled={avatarProcessing}
                    >
                      <Trash2 size={13} />
                      <span>{t.settings.removeAvatar}</span>
                    </button>
                  )}

                  <input
                    ref={fileInputRef}
                    type="file"
                    accept="image/jpeg,image/png,image/webp"
                    style={{ display: "none" }}
                    onChange={handleAvatarFileSelected}
                  />
                </div>

                <div
                  style={{
                    fontSize: 11,
                    color: "var(--emerald)",
                    display: "flex",
                    alignItems: "center",
                    gap: 5,
                    marginTop: 2,
                    lineHeight: 1.4,
                  }}
                >
                  <ShieldCheck size={13} style={{ flexShrink: 0 }} />
                  <span>{t.settings.avatarPrivacyNotice}</span>
                </div>

                {avatarError && (
                  <div style={{ fontSize: 11.5, color: "#FF6B8B" }}>
                    {avatarError}
                  </div>
                )}
              </div>
            </div>

            {/* Display Name and Handle */}
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
              <div>
                <label style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 4, display: "block" }}>
                  {t.onboarding.displayName}
                </label>
                <input
                  className="cyber-input"
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                  placeholder={t.onboarding.displayName}
                />
              </div>
              <div>
                <label style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 4, display: "block" }}>
                  {t.onboarding.handle}
                </label>
                <input
                  className="cyber-input mono"
                  value={handle}
                  onChange={(e) => setHandle(e.target.value)}
                  placeholder={t.onboarding.handle}
                />
              </div>
            </div>

            {/* Role Badge and Avatar Color Theme */}
            <div style={{ display: "grid", gridTemplateColumns: "1fr auto", gap: 10, alignItems: "end" }}>
              <div>
                <label style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 4, display: "block" }}>
                  {t.onboarding.roleBadge}
                </label>
                <input
                  className="cyber-input"
                  value={roleBadge}
                  onChange={(e) => setRoleBadge(e.target.value)}
                  placeholder={t.onboarding.roleBadge}
                />
              </div>
              <div>
                <label style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 4, display: "block" }}>
                  {t.onboarding.avatarColor}
                </label>
                <input
                  type="color"
                  value={avatarColor}
                  onChange={(e) => setAvatarColor(e.target.value)}
                  style={{
                    width: 44,
                    height: 38,
                    background: "transparent",
                    border: "1px solid var(--border-subtle)",
                    borderRadius: 12,
                    cursor: "pointer",
                    display: "block",
                  }}
                />
              </div>
            </div>

            {/* Bio with Live Character Counter */}
            <div>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 4 }}>
                <label style={{ fontSize: 11, color: "var(--text-muted)" }}>
                  {t.settings.bioLabel}
                </label>
                <span style={{ fontSize: 10.5, color: bio.length > 170 ? "#FFB800" : "var(--text-dim)" }}>
                  {bio.length} / 180 {t.settings.bioCharCount}
                </span>
              </div>
              <textarea
                className="cyber-input"
                rows={2}
                maxLength={180}
                value={bio}
                onChange={(e) => setBio(e.target.value)}
                placeholder={t.settings.bioPlaceholder}
                style={{ resize: "vertical", minHeight: 60 }}
              />
            </div>

            <button type="submit" className="cyber-btn" style={{ alignSelf: "flex-start", marginTop: 4 }}>
              {t.settings.saveProfile}
            </button>
          </form>

          <div className="cyber-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={{ fontWeight: 700, fontSize: 13.5, color: "#00F0FF" }}>
              {t.settings.securitySection}
            </div>

            {[
              { checked: stealth, setChecked: setStealth, title: t.settings.stealthTitle, desc: t.settings.stealthDesc },
              { checked: meshRelay, setChecked: setMeshRelay, title: t.settings.meshTitle, desc: t.settings.meshDesc },
              { checked: dhtEnabled, setChecked: setDhtEnabled, title: t.settings.dhtTitle, desc: t.settings.dhtDesc },
              { checked: holePunchEnabled, setChecked: setHolePunchEnabled, title: t.settings.holePunchTitle, desc: t.settings.holePunchDesc },
              { checked: onionRouting, setChecked: setOnionRouting, title: t.settings.onionTitle, desc: t.settings.onionDesc },
              { checked: enforceIranIp, setChecked: setEnforceIranIp, title: t.settings.vpnGuardTitle, desc: t.settings.vpnGuardDesc },
            ].map((item) => (
              <label key={item.title} style={{ display: "flex", alignItems: "flex-start", gap: 9, cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={item.checked}
                  onChange={(e) => item.setChecked(e.target.checked)}
                  style={{ marginTop: 3 }}
                />
                <div>
                  <div style={{ fontSize: 13, fontWeight: 600 }}>{item.title}</div>
                  <div style={{ fontSize: 11.5, color: "var(--text-muted)" }}>{item.desc}</div>
                </div>
              </label>
            ))}

            <button
              type="button"
              className="ghost-btn"
              style={{ alignSelf: "flex-start" }}
              onClick={() => void handleSaveSettings()}
            >
              {t.settings.saveSettings}
            </button>
          </div>

          {/* Main Relay Server Health Check Card */}
          <div
            className="cyber-card"
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 12,
              borderColor: "rgba(0, 240, 255, 0.35)",
              background: "linear-gradient(135deg, rgba(0, 240, 255, 0.05), rgba(10, 18, 32, 0.6))",
              boxShadow: "0 0 20px rgba(0, 240, 255, 0.08)",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 8 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13.5, color: "#00F0FF" }}>
                <Activity size={17} />
                <span>{t.settings.serverHealthTitle}</span>
              </div>
              <button
                type="button"
                className="cyber-btn"
                style={{ padding: "6px 14px", fontSize: 12 }}
                disabled={serverHealthBusy || !serverHealthTarget.trim()}
                onClick={() => void handleTestServerHealth()}
              >
                <Activity size={14} className={serverHealthBusy ? "spin" : ""} />
                <span>{serverHealthBusy ? t.settings.serverHealthTesting : t.settings.serverHealthBtn}</span>
              </button>
            </div>

            <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.55, margin: 0 }}>
              {t.settings.serverHealthDesc}
            </p>

            <input
              className="cyber-input mono"
              dir="ltr"
              aria-label={t.settings.serverHealthTarget}
              placeholder={t.settings.volunteerPlaceholder}
              value={serverHealthTarget}
              disabled={serverHealthBusy}
              onChange={(e) => {
                setServerHealthTarget(e.target.value);
                setServerHealthResult(null);
              }}
            />

            {serverHealthResult && (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 8,
                  padding: 12,
                  borderRadius: 10,
                  fontSize: 12,
                  background: serverHealthResult.reachable ? "rgba(0, 255, 157, 0.08)" : "rgba(255, 56, 100, 0.08)",
                  border: serverHealthResult.reachable
                    ? "1px solid rgba(0, 255, 157, 0.35)"
                    : "1px solid rgba(255, 56, 100, 0.35)",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 6 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 7, fontWeight: 700, color: serverHealthResult.reachable ? "#00FF9D" : "#FF6B8B" }}>
                    <span
                      style={{
                        width: 8,
                        height: 8,
                        borderRadius: "50%",
                        background: serverHealthResult.reachable ? "#00FF9D" : "#FF6B8B",
                        boxShadow: serverHealthResult.reachable ? "0 0 8px #00FF9D" : "0 0 8px #FF6B8B",
                      }}
                    />
                    <span>{serverHealthResult.reachable ? t.settings.serverHealthOnline : t.settings.serverHealthOffline}</span>
                  </div>
                  {serverHealthResult.latency_ms !== undefined && (
                    <span className="mono" style={{ color: "#00F0FF", fontWeight: 700 }}>
                      {serverHealthResult.latency_ms} ms
                    </span>
                  )}
                </div>

                <div style={{ fontSize: 11.5, color: "var(--text-main)", lineHeight: 1.5 }}>
                  {serverHealthResult.message}
                </div>

                {serverHealthResult.reachable && (
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6, fontSize: 11, color: "var(--text-muted)", marginTop: 4 }}>
                    <div>
                      <span>{t.settings.serverHealthHandshake}: </span>
                      <span style={{ color: "#00FF9D", fontWeight: 600 }}>OK (Noise_XX)</span>
                    </div>
                    {serverHealthResult.observed_ip && (
                      <div>
                        <span>{t.settings.serverHealthObservedIp}: </span>
                        <span className="mono" style={{ color: "#00F0FF" }}>{serverHealthResult.observed_ip}</span>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>

          <div
            className="cyber-card"
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 10,
              borderColor: "rgba(0, 255, 157, 0.28)",
              background: "rgba(0, 255, 157, 0.04)",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 7, fontWeight: 700, fontSize: 13.5, color: "#00FF9D" }}>
              <Server size={16} />
              <span>{t.settings.volunteerSection}</span>
            </div>
            <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.55, margin: 0 }}>
              {t.settings.volunteerDesc}
            </p>

            <div style={{ fontSize: 12, fontWeight: 600, color: "#00F0FF", marginTop: 2 }}>
              {t.settings.volunteerStep1Title}
            </div>
            <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.5, margin: 0 }}>
              {t.settings.volunteerStep1Hint}
            </p>
            <pre
              className="mono selectable"
              dir="ltr"
              style={{
                margin: 0,
                padding: 10,
                borderRadius: 10,
                background: "#07090F",
                border: "1px solid var(--border-subtle)",
                fontSize: 10.5,
                color: "#c7f9cc",
                overflowX: "auto",
                maxHeight: 120,
                whiteSpace: "pre",
              }}
            >
              {VOLUNTEER_RELAY_CMD}
            </pre>
            <button
              type="button"
              className="ghost-btn"
              style={{ alignSelf: "flex-start", fontSize: 12 }}
              onClick={() => void handleCopyRelayCmd()}
            >
              {copiedRelayCmd ? <Check size={13} color="#00FF9D" /> : <Terminal size={13} />}
              <span>{copiedRelayCmd ? t.chat.copied : t.settings.volunteerCopyCmd}</span>
            </button>

            <div style={{ fontSize: 12, fontWeight: 600, color: "#00F0FF", marginTop: 4 }}>
              {t.settings.volunteerStep2Title}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "1fr auto", gap: 8 }}>
              <input
                className="cyber-input mono"
                dir="ltr"
                placeholder={t.settings.volunteerPlaceholder}
                value={volunteerAddr}
                onChange={(e) => setVolunteerAddr(e.target.value)}
              />
              <button
                type="button"
                className="cyber-btn"
                disabled={volunteerBusy || !volunteerAddr.trim()}
                onClick={() => void handleRegisterVolunteerRelay()}
              >
                <Plus size={14} />
                <span>{volunteerBusy ? t.settings.volunteerVerifying : t.settings.volunteerSubmitBtn}</span>
              </button>
            </div>
            {volunteerStatus && (
              <div
                style={{
                  fontSize: 11.5,
                  padding: "7px 10px",
                  borderRadius: 9,
                  color: volunteerStatus.ok ? "#00FF9D" : "#FF6B8B",
                  background: volunteerStatus.ok ? "rgba(0,255,157,0.1)" : "rgba(255,56,100,0.1)",
                  border: volunteerStatus.ok
                    ? "1px solid rgba(0,255,157,0.3)"
                    : "1px solid rgba(255,56,100,0.3)",
                }}
              >
                {volunteerStatus.text}
              </div>
            )}
          </div>

          <button
            type="button"
            className="ghost-btn"
            style={{ justifyContent: "space-between" }}
            onClick={() => setShowAdvancedBridge((v) => !v)}
          >
            <span style={{ display: "inline-flex", alignItems: "center", gap: 7, color: "#00FF9D" }}>
              <Sprout size={15} />
              <span>{t.settings.advancedBlackoutToggle}</span>
            </span>
            {showAdvancedBridge ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
          </button>

          {showAdvancedBridge && (
            <div className="cyber-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8, flexWrap: "wrap" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 6, fontWeight: 700, fontSize: 13.5, color: "#00FF9D" }}>
                  <Sprout size={15} />
                  <span>{t.settings.bootstrapSection}</span>
                </div>
                <button
                  type="button"
                  className="ghost-btn"
                  onClick={() => void handleExportSeeds()}
                >
                  <Copy size={13} />
                  <span>{t.settings.exportSeeds}</span>
                </button>
              </div>
              <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.5 }}>
                {t.settings.bootstrapHint}
              </p>
              <div style={{ display: "grid", gridTemplateColumns: "1fr auto", gap: 8 }}>
                <input
                  className="cyber-input mono"
                  placeholder={t.settings.bootstrapPlaceholder}
                  value={newSeedAddr}
                  onChange={(e) => setNewSeedAddr(e.target.value)}
                />
                <button
                  type="button"
                  className="ghost-btn"
                  disabled={!newSeedAddr.trim()}
                  onClick={() => void handleAddBootstrap()}
                >
                  <Plus size={14} />
                  <span>{t.settings.bootstrapAdd}</span>
                </button>
              </div>
              {bootstrapPeers.length > 0 && (
                <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                  {bootstrapPeers.map((addr) => (
                    <span
                      key={addr}
                      className="mono"
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 6,
                        fontSize: 11,
                        padding: "4px 8px",
                        borderRadius: 8,
                        background: "rgba(0,240,255,0.08)",
                        border: "1px solid rgba(0,240,255,0.22)",
                      }}
                    >
                      <span>{addr}</span>
                      <button
                        type="button"
                        onClick={() => void handleRemoveBootstrap(addr)}
                        style={{ background: "none", border: "none", color: "#FF6B8B", cursor: "pointer", padding: 0, display: "flex" }}
                      >
                        <Trash2 size={12} />
                      </button>
                    </span>
                  ))}
                </div>
              )}
              <div style={{ display: "grid", gridTemplateColumns: "1fr auto", gap: 8 }}>
                <input
                  className="cyber-input mono"
                  placeholder={t.settings.importSeedsPlaceholder}
                  value={seedBundleInput}
                  onChange={(e) => setSeedBundleInput(e.target.value)}
                />
                <button
                  type="button"
                  className="ghost-btn"
                  disabled={!seedBundleInput.trim()}
                  onClick={() => void handleImportSeeds()}
                >
                  <span>{t.settings.importSeedsBtn}</span>
                </button>
              </div>
            </div>
          )}

          <div className="cyber-card" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div style={{ fontWeight: 700, fontSize: 13.5, color: "#FFD866" }}>
              {t.settings.duressSection}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr auto", gap: 8 }}>
              <input
                type="password"
                className="cyber-input"
                placeholder={t.settings.duressPinPlaceholder}
                value={duressPin}
                onChange={(e) => setDuressPin(e.target.value)}
              />
              <select
                className="cyber-input"
                value={duressAction}
                onChange={(e) => setDuressAction(e.target.value as DuressAction)}
              >
                <option value="decoy">{t.onboarding.duressModeDecoy}</option>
                <option value="wipe">{t.onboarding.duressModeWipe}</option>
              </select>
              <button
                type="button"
                className="ghost-btn"
                disabled={duressPin.trim().length < 4}
                onClick={() => void handleSetDuress()}
              >
                {t.settings.saveDuress}
              </button>
            </div>
          </div>

          {platform === "windows" && (
            <div className="cyber-card" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13.5, color: "#00FF9D" }}>
                <Laptop size={16} />
                <span>{t.settings.windowsSection}</span>
              </div>
              <p style={{ fontSize: 11.5, color: "var(--text-muted)" }}>{t.settings.windowsDesc}</p>
              <button
                type="button"
                className="cyber-btn"
                style={{ alignSelf: "flex-start" }}
                onClick={() => void handleInstallWindows()}
              >
                {t.settings.windowsInstallBtn}
              </button>
            </div>
          )}

          <div
            className="cyber-card"
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 10,
              borderColor: "rgba(255, 56, 100, 0.35)",
              background: "rgba(255, 56, 100, 0.04)",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 13.5, color: "#FF6B8B" }}>
              <RotateCcw size={16} />
              <span>{t.settings.factoryResetSection}</span>
            </div>
            <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.55, margin: 0 }}>
              {t.settings.factoryResetDesc}
            </p>

            {!showResetConfirm ? (
              <button
                type="button"
                className="ghost-btn"
                style={{
                  alignSelf: "flex-start",
                  color: "#FF6B8B",
                  borderColor: "rgba(255, 56, 100, 0.4)",
                  fontSize: 12,
                }}
                onClick={() => setShowResetConfirm(true)}
              >
                <Trash2 size={14} />
                <span>{t.settings.factoryResetBtn}</span>
              </button>
            ) : (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 8,
                  padding: 10,
                  borderRadius: 10,
                  background: "rgba(255, 56, 100, 0.08)",
                  border: "1px solid rgba(255, 56, 100, 0.3)",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12.5, fontWeight: 700, color: "#FF6B8B" }}>
                  <AlertTriangle size={15} />
                  <span>{t.settings.factoryResetConfirmTitle}</span>
                </div>
                <p style={{ fontSize: 11.5, color: "var(--text-muted)", lineHeight: 1.5, margin: 0 }}>
                  {t.settings.factoryResetConfirmDesc}
                </p>
                <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
                  <button
                    type="button"
                    className="cyber-btn"
                    disabled={resetting}
                    style={{
                      background: "linear-gradient(135deg, #FF3864 0%, #B80036 100%)",
                      borderColor: "#FF3864",
                      fontSize: 12,
                      flex: 1,
                    }}
                    onClick={() => void handleFactoryReset()}
                  >
                    <Trash2 size={13} />
                    <span>{resetting ? "..." : t.settings.factoryResetConfirmBtn}</span>
                  </button>
                  <button
                    type="button"
                    className="ghost-btn"
                    disabled={resetting}
                    style={{ fontSize: 12 }}
                    onClick={() => setShowResetConfirm(false)}
                  >
                    <span>{t.common.close}</span>
                  </button>
                </div>
              </div>
            )}
          </div>

          {telemetry && (
            <div className="cyber-card">
              <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 8 }}>
                {t.settings.telemetryTitle}
              </div>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 1fr",
                  gap: 8,
                  fontSize: 12,
                }}
              >
                <div>
                  <span style={{ color: "var(--text-muted)" }}>{t.settings.listeningPort}: </span>
                  <span className="mono" style={{ color: "#00F0FF" }}>
                    {telemetry.node_listening_port}
                  </span>
                </div>
                <div>
                  <span style={{ color: "var(--text-muted)" }}>{t.settings.activePeers}: </span>
                  <span className="mono" style={{ color: "#00FF9D" }}>
                    {telemetry.active_direct_peers}
                  </span>
                </div>
                <div>
                  <span style={{ color: "var(--text-muted)" }}>{t.settings.queuedMsgs}: </span>
                  <span className="mono">{telemetry.queued_offline_messages}</span>
                </div>
                <div>
                  <span style={{ color: "var(--text-muted)" }}>{t.settings.relayedPkts}: </span>
                  <span className="mono">{telemetry.mesh_relayed_packets}</span>
                </div>
                <div>
                  <span style={{ color: "var(--text-muted)" }}>{t.settings.dhtNodes}: </span>
                  <span className="mono" style={{ color: "#00F0FF" }}>{telemetry.dht_known_nodes}</span>
                </div>
                <div>
                  <span style={{ color: "var(--text-muted)" }}>{t.settings.dhtRecords}: </span>
                  <span className="mono">{telemetry.dht_stored_records}</span>
                </div>
                <div>
                  <span style={{ color: "var(--text-muted)" }}>{t.settings.punchSuccesses}: </span>
                  <span className="mono" style={{ color: "#00FF9D" }}>{telemetry.nat_punch_successes}</span>
                </div>
                <div>
                  <span style={{ color: "var(--text-muted)" }}>{t.settings.onionPkts}: </span>
                  <span className="mono" style={{ color: "#c084fc" }}>{telemetry.onion_routed_packets}</span>
                </div>
                {telemetry.observed_external_addr && (
                  <div style={{ gridColumn: "1 / -1" }}>
                    <span style={{ color: "var(--text-muted)" }}>{t.settings.observedExtAddr}: </span>
                    <span className="mono selectable" style={{ color: "#c084fc" }}>
                      {telemetry.observed_external_addr}
                    </span>
                  </div>
                )}
              </div>
            </div>
          )}

          <div
            className="cyber-card"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 14,
              background: "rgba(0, 240, 255, 0.03)",
              borderColor: "rgba(0, 240, 255, 0.2)",
              padding: "12px 14px",
            }}
          >
            <div
              style={{
                width: 60,
                height: 60,
                borderRadius: 16,
                background: "radial-gradient(circle at 50% 50%, rgba(0, 240, 255, 0.22), rgba(10, 18, 32, 0.9))",
                border: "1.5px solid rgba(0, 240, 255, 0.45)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                padding: 8,
                flexShrink: 0,
                boxShadow: "0 0 18px rgba(0, 240, 255, 0.25)",
              }}
            >
              <img
                src="/cymesh-cyber-logo.png"
                alt="Cymesh"
                style={{
                  width: "100%",
                  height: "100%",
                  objectFit: "contain",
                  filter: "drop-shadow(0 0 8px #00F0FF)",
                }}
              />
            </div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span style={{ fontSize: 13.5, fontWeight: 700, color: "#00F0FF" }}>
                  Cymesh v2.1
                </span>
                <span
                  className="mono"
                  style={{
                    fontSize: 10,
                    padding: "1px 6px",
                    borderRadius: 6,
                    background: "rgba(0, 255, 157, 0.12)",
                    color: "#00FF9D",
                    border: "1px solid rgba(0, 255, 157, 0.3)",
                  }}
                >
                  ZERO-SERVER E2EE
                </span>
              </div>
              <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
                P2P Blind Mesh • Double Ratchet Forward Secrecy • ChaCha20-Poly1305
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

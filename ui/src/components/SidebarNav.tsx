import React from "react";
import {
  Eye,
  EyeOff,
  Globe,
  Lock,
  MessageSquare,
  Radio,
  Settings,
  ShieldCheck,
  UserPlus,
  Zap,
} from "lucide-react";
import type { AppLang } from "../i18n";
import type { TranslationDict } from "../i18n/en";
import type { SignedProfileCard } from "../types/aegis";
import { CyberAvatar } from "./CyberAvatar";

export type ChatFilter = "all" | "direct" | "mesh" | "verified";

interface SidebarNavProps {
  t: TranslationDict;
  lang: AppLang;
  ownProfile: SignedProfileCard | null;
  filter: ChatFilter;
  stealthMode: boolean;
  onChangeFilter: (f: ChatFilter) => void;
  onToggleStealth: () => void;
  onOpenPair: () => void;
  onOpenSettings: () => void;
  onToggleLang: () => void;
  onLockVault: () => void;
}

export const SidebarNav: React.FC<SidebarNavProps> = ({
  t,
  lang,
  ownProfile,
  filter,
  stealthMode,
  onChangeFilter,
  onToggleStealth,
  onOpenPair,
  onOpenSettings,
  onToggleLang,
  onLockVault,
}) => {
  return (
    <aside className="rail-nav">
      <button
        type="button"
        onClick={onOpenSettings}
        title={t.appName}
        style={{
          background: "radial-gradient(circle at 50% 50%, rgba(0, 240, 255, 0.22), rgba(11, 23, 47, 0.9))",
          border: "1.5px solid rgba(0, 240, 255, 0.45)",
          borderRadius: 14,
          width: 44,
          height: 44,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          padding: 5,
          cursor: "pointer",
          marginBottom: 10,
          boxShadow: "0 0 14px rgba(0, 240, 255, 0.22)",
        }}
      >
        <img
          src="/cymesh-cyber-logo.png"
          alt="Cymesh"
          style={{
            width: "100%",
            height: "100%",
            objectFit: "contain",
            filter: "drop-shadow(0 0 6px #00F0FF)",
          }}
        />
      </button>

      <button
        type="button"
        onClick={onOpenSettings}
        title={ownProfile?.display_name ?? t.appName}
        style={{ background: "transparent", border: "none", cursor: "pointer", marginBottom: 6 }}
      >
        <CyberAvatar
          name={ownProfile?.display_name ?? "A"}
          color={ownProfile?.avatar_color ?? "#00F0FF"}
          avatarUrl={ownProfile?.avatar_data}
          size={42}
          online={!stealthMode}
        />
      </button>

      <button
        type="button"
        className={`rail-btn ${filter === "all" ? "active" : ""}`}
        onClick={() => onChangeFilter("all")}
      >
        <MessageSquare size={18} />
        <span>{t.nav.all}</span>
      </button>

      <button
        type="button"
        className={`rail-btn ${filter === "direct" ? "active" : ""}`}
        onClick={() => onChangeFilter("direct")}
      >
        <Zap size={18} />
        <span>{t.nav.direct}</span>
      </button>

      <button
        type="button"
        className={`rail-btn ${filter === "mesh" ? "active" : ""}`}
        onClick={() => onChangeFilter("mesh")}
      >
        <Radio size={18} />
        <span>{t.nav.mesh}</span>
      </button>

      <button
        type="button"
        className={`rail-btn ${filter === "verified" ? "active" : ""}`}
        onClick={() => onChangeFilter("verified")}
      >
        <ShieldCheck size={18} />
        <span>{t.nav.verified}</span>
      </button>

      <div style={{ flex: 1 }} />

      <button type="button" className="rail-btn" onClick={onOpenPair}>
        <UserPlus size={18} />
        <span>{t.nav.addPeer}</span>
      </button>

      <button
        type="button"
        className={`rail-btn ${stealthMode ? "active" : ""}`}
        onClick={onToggleStealth}
      >
        {stealthMode ? <EyeOff size={18} /> : <Eye size={18} />}
        <span>{stealthMode ? t.nav.stealthOn : t.nav.stealthOff}</span>
      </button>

      <button type="button" className="rail-btn" onClick={onToggleLang}>
        <Globe size={18} />
        <span>{lang === "fa" ? "EN" : "FA"}</span>
      </button>

      <button type="button" className="rail-btn" onClick={onOpenSettings}>
        <Settings size={18} />
        <span>{t.nav.settings}</span>
      </button>

      <button type="button" className="rail-btn danger" onClick={onLockVault}>
        <Lock size={18} />
        <span>{t.nav.lockNow}</span>
      </button>
    </aside>
  );
};

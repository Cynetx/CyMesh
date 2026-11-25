import React, { useEffect, useState } from "react";
import { ArrowRight, ShieldCheck, Terminal } from "lucide-react";
import { api } from "../api/bridge";
import type { TranslationDict } from "../i18n/en";
import type { NetworkTelemetry } from "../types/aegis";

interface MeshStartupProps {
  t: TranslationDict;
  accountId: string | null;
  onFinish: () => void;
}

export const MeshStartup: React.FC<MeshStartupProps> = ({
  t,
  accountId,
  onFinish,
}) => {
  const [stage, setStage] = useState(0);
  const [progress, setProgress] = useState(12);
  const [tel, setTel] = useState<NetworkTelemetry | null>(null);

  useEffect(() => {
    let cancelled = false;

    const runUplink = async () => {
      // Animate startup while the unlocked session is displayed
      await new Promise((r) => setTimeout(r, 340));
      if (cancelled) return;
      setStage(1);
      setProgress(34);

      await new Promise((r) => setTimeout(r, 360));
      if (cancelled) return;
      setStage(2);
      setProgress(58);

      // Request fresh peer discovery and telemetry
      await api.triggerDiscoveryPulse().catch(() => {});
      const [latestTel] = await Promise.all([
        api.getNetworkTelemetry().catch(() => null),
        api.searchAccounts("").catch(() => []),
      ]);
      if (cancelled) return;
      if (latestTel) setTel(latestTel);
      setStage(3);
      setProgress(82);

      await new Promise((r) => setTimeout(r, 450));
      if (cancelled) return;
      setStage(4);
      setProgress(100);

      await new Promise((r) => setTimeout(r, 620));
      if (!cancelled) onFinish();
    };

    void runUplink();
    return () => {
      cancelled = true;
    };
  }, [onFinish]);

  const lines = [
    { idx: "01", text: t.boot.stepCrypto, done: stage >= 0 },
    { idx: "02", text: t.boot.stepVault, done: stage >= 1 },
    { idx: "03", text: t.boot.stepTransport, done: stage >= 2 },
    {
      idx: "04",
      text: `${t.boot.stepMesh}${
        tel
          ? ` [${tel.dht_known_nodes} ${t.boot.nodesFound}]`
          : ""
      }`,
      done: stage >= 3,
    },
    {
      idx: "05",
      text: `${t.boot.stepReady}${
        tel?.account_id || accountId
          ? ` (${t.boot.accountLive}: ${tel?.account_id ?? accountId})`
          : ""
      }`,
      done: stage >= 4,
    },
  ];

  return (
    <div className="wd-boot-shell" dir="ltr">
      <div className="wd-scanlines" />

      <div
        style={{
          position: "relative",
          zIndex: 2,
          width: "100%",
          maxWidth: 640,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          gap: 20,
        }}
      >
        <div
          className="mono"
          style={{
            fontSize: 11,
            color: "#00F0FF",
            letterSpacing: "0.14em",
            opacity: 0.85,
            textAlign: "center",
          }}
        >
          {t.boot.headerTag}
        </div>

        {/* Animated mesh diagram */}
        <div className="wd-pulse" style={{ position: "relative", width: 148, height: 148, display: "flex", alignItems: "center", justifyContent: "center" }}>
          <img
            src="/cymesh-cyber-logo.png"
            alt="Cymesh"
            style={{
              position: "absolute",
              width: 52,
              height: 52,
              objectFit: "contain",
              zIndex: 3,
              filter: stage >= 4 ? "drop-shadow(0 0 14px #00FF9D)" : "drop-shadow(0 0 14px #00F0FF)",
              transition: "filter 0.5s ease",
            }}
          />
          <svg viewBox="0 0 160 160" width="148" height="148">
            <polygon
              points="80,10 140,45 140,115 80,150 20,115 20,45"
              fill="rgba(0,240,255,0.04)"
              stroke="rgba(0,240,255,0.55)"
              strokeWidth="1.6"
              strokeDasharray="10 5"
            />
            <polygon
              points="80,26 126,53 126,107 80,134 34,107 34,53"
              fill="none"
              stroke={stage >= 4 ? "#00FF9D" : "#FF2A85"}
              strokeWidth="1.2"
              opacity="0.8"
            />
            <line x1="80" y1="26" x2="80" y2="134" stroke="rgba(0,240,255,0.28)" strokeWidth="1" />
            <line x1="34" y1="53" x2="126" y2="107" stroke="rgba(0,240,255,0.28)" strokeWidth="1" />
            <line x1="126" y1="53" x2="34" y2="107" stroke="rgba(0,240,255,0.28)" strokeWidth="1" />
            <circle cx="80" cy="80" r="28" fill="rgba(7,9,15,0.7)" stroke={stage >= 4 ? "#00FF9D" : "#00F0FF"} strokeWidth="1.2" />
            <circle cx="80" cy="26" r="4" fill="#00F0FF" />
            <circle cx="126" cy="53" r="4" fill="#00FF9D" />
            <circle cx="126" cy="107" r="4" fill="#00F0FF" />
            <circle cx="80" cy="134" r="4" fill="#FF2A85" />
            <circle cx="34" cy="107" r="4" fill="#00FF9D" />
            <circle cx="34" cy="53" r="4" fill="#00F0FF" />
          </svg>
        </div>

        <div style={{ textAlign: "center" }}>
          <div className="wd-glitch-title" data-text="CYMESH">
            CYMESH
          </div>
          <div
            className="mono"
            style={{
              fontSize: 11,
              color: "#8A9BB8",
              letterSpacing: "0.18em",
              marginTop: 6,
            }}
          >
            {t.boot.subtitle}
          </div>
        </div>

        <div
          className="cyber-card"
          style={{
            width: "100%",
            background: "rgba(9, 13, 24, 0.92)",
            border: "1px solid rgba(0, 240, 255, 0.3)",
            boxShadow: "0 0 36px rgba(0, 240, 255, 0.12)",
            padding: 18,
            display: "flex",
            flexDirection: "column",
            gap: 10,
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              borderBottom: "1px solid rgba(0, 240, 255, 0.14)",
              paddingBottom: 8,
              fontSize: 11.5,
              color: "#00F0FF",
            }}
            className="mono"
          >
            <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
              <Terminal size={14} />
              <span>{t.boot.headerTag}</span>
            </span>
            <span>{progress}%</span>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
            {lines.map((line, i) => {
              const visible = stage >= i;
              const current = stage === i && stage < 4;
              return (
                <div
                  key={line.idx}
                  className="mono"
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    fontSize: 12,
                    color: visible ? (i === 4 ? "#00FF9D" : "#E6EDF7") : "#3E4C63",
                    opacity: visible ? 1 : 0.45,
                    transition: "opacity 0.2s ease, color 0.2s ease",
                  }}
                >
                  <span style={{ color: current ? "#FF2A85" : "#00F0FF" }}>
                    [{line.idx}]
                  </span>
                  <span style={{ flex: 1 }}>{line.text}</span>
                  {visible && !current && <ShieldCheck size={14} color="#00FF9D" />}
                </div>
              );
            })}
          </div>

          <div
            style={{
              height: 6,
              borderRadius: 999,
              background: "rgba(255,255,255,0.06)",
              overflow: "hidden",
              marginTop: 6,
            }}
          >
            <div
              style={{
                height: "100%",
                width: `${progress}%`,
                background: "linear-gradient(90deg, #00F0FF, #00FF9D)",
                boxShadow: "0 0 12px rgba(0, 240, 255, 0.7)",
                transition: "width 0.35s ease",
              }}
            />
          </div>
        </div>

        <button
          type="button"
          className="ghost-btn"
          onClick={onFinish}
          style={{ borderColor: "rgba(0,240,255,0.28)", color: "#8A9BB8" }}
        >
          <span>{t.boot.skipBtn}</span>
          <ArrowRight size={14} />
        </button>
      </div>
    </div>
  );
};

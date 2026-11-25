import React, { useState } from "react";
import {
  ShieldAlert,
  RotateCw,
  WifiOff,
  GlobeLock,
} from "lucide-react";
import type { VpnGuardStatus } from "../utils/vpnGuard";

interface VpnGuardBlockScreenProps {
  guardStatus: VpnGuardStatus;
  onRetry: () => Promise<void>;
  lang?: "fa" | "en";
}

export const VpnGuardBlockScreen: React.FC<VpnGuardBlockScreenProps> = ({
  guardStatus,
  onRetry,
  lang = "fa",
}) => {
  const [retrying, setRetrying] = useState(false);

  const handleRetry = async () => {
    if (retrying) return;
    setRetrying(true);
    try {
      await onRetry();
    } finally {
      setRetrying(false);
    }
  };

  const isFa = lang === "fa";

  const getReasonTitle = () => {
    switch (guardStatus.reason) {
      case "VPN_ADAPTER":
        return isFa
          ? "رابط شبکه مجازی / فیلترشکن فعال است"
          : "Active VPN / TUN Interface Detected";
      case "PROXY":
        return isFa ? "پروکسی سیستمی شناسایی شد" : "System Proxy Active";
      case "FOREIGN_IP":
        return isFa
          ? "آی‌پی خارج از ایران شناسایی شد"
          : "Non-Iran Public IP Detected";
      default:
        return isFa ? "شناسایی ارتباط غیرمجاز" : "Unauthorized Network Path";
    }
  };

  const getReasonDetail = () => {
    switch (guardStatus.reason) {
      case "VPN_ADAPTER":
        return guardStatus.adapterName
          ? `${isFa ? "آداپتور:" : "Adapter:"} ${guardStatus.adapterName}`
          : isFa
          ? "آداپتور مجازی فیلترشکن (Wintun / WireGuard / TAP / TUN)"
          : "Virtual Network Tunnel Adapter";
      case "PROXY":
        return guardStatus.proxyDetails
          ? `${isFa ? "تنظیمات پروکسی:" : "Proxy:"} ${guardStatus.proxyDetails}`
          : isFa
          ? "پروکسی سیستم فعال است"
          : "System Proxy configured";
      case "FOREIGN_IP":
        return `${isFa ? "کشور شناسایی‌شده:" : "Country:"} ${
          guardStatus.detectedCountry || "Unknown"
        } ${guardStatus.detectedIp ? `(${guardStatus.detectedIp})` : ""}`;
      default:
        return null;
    }
  };

  return (
    <div
      dir={isFa ? "rtl" : "ltr"}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 99999,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background:
          "radial-gradient(circle at 50% 30%, rgba(255, 0, 85, 0.14) 0%, rgba(10, 14, 23, 0.98) 70%, #06090f 100%)",
        backdropFilter: "blur(24px)",
        color: "#E2E8F0",
        padding: "24px",
        overflowY: "auto",
      }}
    >
      <div
        className="cyber-card"
        style={{
          maxWidth: 540,
          width: "100%",
          padding: "36px 32px",
          background: "rgba(18, 22, 34, 0.88)",
          border: "1px solid rgba(255, 0, 85, 0.35)",
          borderRadius: 24,
          boxShadow:
            "0 20px 60px rgba(0, 0, 0, 0.8), 0 0 40px rgba(255, 0, 85, 0.18)",
          textAlign: "center",
          position: "relative",
          animation: "cyberPulse 4s ease-in-out infinite alternate",
        }}
      >
        {/* Shield Icon with glowing radar pulse */}
        <div
          style={{
            position: "relative",
            width: 88,
            height: 88,
            margin: "0 auto 20px auto",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: "50%",
            background:
              "radial-gradient(circle, rgba(255, 0, 85, 0.25) 0%, rgba(255, 0, 85, 0.05) 70%)",
            border: "1px solid rgba(255, 0, 85, 0.5)",
            boxShadow: "0 0 30px rgba(255, 0, 85, 0.35)",
          }}
        >
          <ShieldAlert size={46} color="#FF0055" />
        </div>

        {/* Badge */}
        <div
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: "4px 14px",
            borderRadius: 20,
            background: "rgba(255, 0, 85, 0.15)",
            border: "1px solid rgba(255, 0, 85, 0.4)",
            color: "#FF3366",
            fontSize: 12,
            fontWeight: 700,
            letterSpacing: "0.04em",
            marginBottom: 16,
          }}
        >
          <GlobeLock size={14} />
          <span>
            {isFa
              ? "سپر تفکیک اختلال و پایش شبکه"
              : "VPN & Regional Guard Active"}
          </span>
        </div>

        {/* Title */}
        <h2
          style={{
            fontSize: 21,
            fontWeight: 800,
            color: "#FFFFFF",
            marginBottom: 12,
            letterSpacing: isFa ? 0 : "-0.02em",
          }}
        >
          {isFa
            ? "لطفاً فیلترشکن خود را خاموش کنید"
            : "Please Disconnect Your VPN / Proxy"}
        </h2>

        {/* Subtitle / Explanation */}
        <p
          style={{
            fontSize: 13.5,
            lineHeight: 1.7,
            color: "var(--text-muted, #94A3B8)",
            marginBottom: 22,
          }}
        >
          {isFa
            ? "برای تشخیص دقیق اختلالات شبکه از باگ‌های نرم‌افزاری و برقراری ارتباط پایدار P2P همتا‌به‌همتا، ورود به سای‌مش تنها از طریق اینترنت مستقیم ایران مجاز است."
            : "To isolate network disruptions from software issues and ensure direct P2P mesh connectivity, Cymesh is restricted to direct Iran network connections without VPNs."}
        </p>

        {/* Detection Details Box */}
        <div
          style={{
            padding: "14px 16px",
            borderRadius: 14,
            background: "rgba(10, 14, 23, 0.7)",
            border: "1px solid rgba(255, 255, 255, 0.08)",
            marginBottom: 24,
            textAlign: isFa ? "right" : "left",
          }}
        >
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              fontSize: 12.5,
              fontWeight: 700,
              color: "#FF5577",
              marginBottom: 4,
            }}
          >
            <WifiOff size={15} />
            <span>{getReasonTitle()}</span>
          </div>

          {getReasonDetail() && (
            <div
              className="mono"
              style={{
                fontSize: 12,
                color: "#E2E8F0",
                wordBreak: "break-all",
                marginTop: 4,
              }}
            >
              {getReasonDetail()}
            </div>
          )}
        </div>

        {/* Steps */}
        <div
          style={{
            textAlign: isFa ? "right" : "left",
            fontSize: 12.5,
            color: "#94A3B8",
            lineHeight: 1.8,
            marginBottom: 28,
            padding: "0 8px",
          }}
        >
          <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
            <span style={{ color: "#00F0FF", fontWeight: 700 }}>۱.</span>
            <span>
              {isFa
                ? "نرم‌افزار فیلترشکن یا پروکسی فعال روی سیستم خود را کاملاً قطع کنید (Disconnect)."
                : "Completely disconnect your VPN client or system proxy."}
            </span>
          </div>
          <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
            <span style={{ color: "#00F0FF", fontWeight: 700 }}>۲.</span>
            <span>
              {isFa
                ? "چند ثانیه صبر کنید تا آداپتور مجازی ویندوز بسته شده و آی‌پی ملی شما فعال شود."
                : "Wait a few seconds for the virtual adapter to close and your direct IP to become active."}
            </span>
          </div>
          <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
            <span style={{ color: "#00F0FF", fontWeight: 700 }}>۳.</span>
            <span>
              {isFa
                ? "دکمه زیر را فشار دهید تا اتصال مجدداً بررسی و ورود تأیید شود."
                : "Click the button below to re-verify your connection and proceed."}
            </span>
          </div>
        </div>

        {/* Retry Button */}
        <button
          type="button"
          className="cyber-btn primary"
          onClick={handleRetry}
          disabled={retrying}
          style={{
            width: "100%",
            padding: "13px 20px",
            fontSize: 14.5,
            fontWeight: 700,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 10,
            cursor: retrying ? "wait" : "pointer",
            background: "linear-gradient(135deg, #00F0FF 0%, #0099FF 100%)",
            color: "#050811",
            border: "none",
            borderRadius: 14,
            boxShadow: "0 4px 20px rgba(0, 240, 255, 0.35)",
            transition: "all 0.2s ease",
          }}
        >
          <RotateCw
            size={18}
            style={{
              animation: retrying ? "spin 1s linear infinite" : "none",
            }}
          />
          <span>
            {retrying
              ? isFa
                ? "در حال بررسی وضعیت شبکه..."
                : "Checking network..."
              : isFa
              ? "بررسی مجدد اتصال"
              : "Re-check Connection"}
          </span>
        </button>
      </div>
    </div>
  );
};

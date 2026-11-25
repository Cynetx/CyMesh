import React, { useState } from "react";
import { isSafeAvatarUrl } from "../utils/avatarSecurity";

interface CyberAvatarProps {
  name: string;
  color: string;
  avatarUrl?: string | null;
  size?: number;
  online?: boolean;
  verified?: boolean;
  className?: string;
  onClick?: () => void;
}

export const CyberAvatar: React.FC<CyberAvatarProps> = ({
  name,
  color,
  avatarUrl,
  size = 42,
  online = false,
  verified = false,
  className,
  onClick,
}) => {
  const [imgError, setImgError] = useState(false);

  const initials =
    name
      .trim()
      .split(/\s+/)
      .slice(0, 2)
      .map((w) => w[0] ?? "")
      .join("")
      .toUpperCase() || "A";

  const accent = color || "#00F0FF";
  const hasValidPhoto = Boolean(avatarUrl && isSafeAvatarUrl(avatarUrl) && !imgError);

  return (
    <div
      className={className}
      onClick={onClick}
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.34),
        background: hasValidPhoto
          ? "#0A0E1A"
          : `radial-gradient(circle at 30% 25%, ${accent}44, #0a101f 85%)`,
        border: `1.5px solid ${accent}88`,
        boxShadow: online ? `0 0 14px ${accent}33` : "none",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        position: "relative",
        flexShrink: 0,
        fontWeight: 700,
        fontSize: Math.max(12, Math.round(size * 0.36)),
        color: "#EAF2FF",
        letterSpacing: "0.03em",
        overflow: "visible",
      }}
    >
      {hasValidPhoto ? (
        <img
          src={avatarUrl!}
          alt={name}
          onError={() => setImgError(true)}
          style={{
            width: "100%",
            height: "100%",
            objectFit: "cover",
            borderRadius: Math.round(size * 0.32),
            display: "block",
          }}
        />
      ) : (
        initials
      )}

      {online && (
        <span
          style={{
            position: "absolute",
            bottom: -2,
            insetInlineEnd: -2,
            width: Math.max(9, Math.round(size * 0.25)),
            height: Math.max(9, Math.round(size * 0.25)),
            borderRadius: "999px",
            background: "#00FF9D",
            border: "2px solid #0B0F19",
            boxShadow: "0 0 8px #00FF9D",
            zIndex: 2,
          }}
        />
      )}
      {verified && (
        <span
          style={{
            position: "absolute",
            top: -3,
            insetInlineEnd: -3,
            width: 14,
            height: 14,
            borderRadius: "999px",
            background: "#00F0FF",
            color: "#07090F",
            fontSize: 9,
            fontWeight: 900,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            border: "1.5px solid #07090F",
            zIndex: 2,
          }}
        >
          ✓
        </span>
      )}
    </div>
  );
};

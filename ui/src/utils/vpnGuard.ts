import { api } from "../api/bridge";

export interface VpnGuardStatus {
  blocked: boolean;
  reason?: "VPN_ADAPTER" | "PROXY" | "FOREIGN_IP";
  adapterName?: string;
  detectedCountry?: string;
  detectedIp?: string;
  proxyDetails?: string;
  checkedAt: number;
}

interface GeoIpResult {
  country: string;
  ip: string;
}

async function fetchWithTimeout(url: string, timeoutMs = 3500): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(url, {
      signal: controller.signal,
      cache: "no-store",
    });
    return res;
  } finally {
    clearTimeout(timer);
  }
}

async function detectPublicGeoIp(): Promise<GeoIpResult | null> {
  // Provider 1: api.country.is
  try {
    const res = await fetchWithTimeout("https://api.country.is/", 3000);
    if (res.ok) {
      const data = await res.json();
      if (typeof data?.country === "string" && typeof data?.ip === "string") {
        return {
          country: data.country.toUpperCase(),
          ip: data.ip,
        };
      }
    }
  } catch {
    // Fallback to next provider
  }

  // Provider 2: ipwho.is
  try {
    const res = await fetchWithTimeout("https://ipwho.is/", 3500);
    if (res.ok) {
      const data = await res.json();
      if (data?.success && typeof data?.country_code === "string") {
        return {
          country: data.country_code.toUpperCase(),
          ip: data.ip || "",
        };
      }
    }
  } catch {
    // Fallback to next provider
  }

  return null;
}

/**
 * Evaluates whether the environment has an active VPN/Proxy or is originating outside Iran.
 * @param enforceIranOnly When true, queries public GeoIP and verifies IP country is "IR".
 */
export async function evaluateVpnGuard(enforceIranOnly = false): Promise<VpnGuardStatus> {
  const now = Date.now();

  // 1. Check local network adapters for active VPN / TUN / TAP interfaces & env proxies
  try {
    const adapterInfo = await api.checkVpnAdapters();
    if (adapterInfo.vpn_detected) {
      return {
        blocked: true,
        reason: "VPN_ADAPTER",
        adapterName: adapterInfo.adapter_name ?? undefined,
        checkedAt: now,
      };
    }
    if (adapterInfo.proxy_detected) {
      return {
        blocked: true,
        reason: "PROXY",
        proxyDetails: adapterInfo.proxy_details ?? undefined,
        checkedAt: now,
      };
    }
  } catch (err) {
    console.warn("Failed to inspect local network adapters via Rust backend:", err);
  }

  // 2. Check public IP Geolocation if Iran enforcement is active
  if (enforceIranOnly) {
    try {
      const geo = await detectPublicGeoIp();
      if (geo && geo.country) {
        if (geo.country !== "IR") {
          return {
            blocked: true,
            reason: "FOREIGN_IP",
            detectedCountry: geo.country,
            detectedIp: geo.ip,
            checkedAt: now,
          };
        }
        return {
          blocked: false,
          detectedCountry: geo.country,
          detectedIp: geo.ip,
          checkedAt: now,
        };
      }
    } catch (err) {
      console.warn("GeoIP check encountered an error:", err);
    }
  }

  // If offline/intranet and no VPN adapter detected, allow LAN mesh communication
  return {
    blocked: false,
    checkedAt: now,
  };
}

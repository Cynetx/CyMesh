import type { TranslationDict } from "../i18n/en";
import type { ConnectionRoute } from "../types/aegis";

export function isDirectRoute(route: ConnectionRoute): boolean {
  return route === "lan_direct" || route === "wan_direct";
}

export function isOnlineRoute(route: ConnectionRoute): boolean {
  return route !== "offline";
}

export function routePillClass(route: ConnectionRoute): string {
  switch (route) {
    case "lan_direct":
    case "wan_direct":
      return "wan";
    case "mesh_relay":
      return "mesh";
    case "offline":
      return "offline";
  }
}

export function formatRouteLabel(
  route: ConnectionRoute,
  t: TranslationDict,
  latencyMs?: number | null,
  compactLatencyOnly = false,
): string {
  switch (route) {
    case "lan_direct":
    case "wan_direct":
      if (compactLatencyOnly && latencyMs) return `${latencyMs}ms`;
      return latencyMs ? `${t.chatList.directP2P} • ${latencyMs}ms` : t.chatList.directP2P;
    case "mesh_relay":
      return t.chatList.meshRelay;
    case "offline":
      return t.chatList.offline;
  }
}

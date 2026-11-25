import { en, type TranslationDict } from "./en";
import { fa } from "./fa";

export type AppLang = "fa" | "en";

export const dictionaries: Record<AppLang, TranslationDict> = {
  fa,
  en,
};

export function getDirection(lang: AppLang): "rtl" | "ltr" {
  return lang === "fa" ? "rtl" : "ltr";
}

export function formatTime(ts: number, lang: AppLang): string {
  try {
    return new Intl.DateTimeFormat(lang === "fa" ? "fa-IR" : "en-US", {
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(ts));
  } catch {
    return "";
  }
}

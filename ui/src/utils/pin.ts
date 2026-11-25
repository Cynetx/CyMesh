/**
 * Normalizes user PIN/Password input:
 * 1. Trims leading and trailing whitespace.
 * 2. Converts Persian digits (۰-۹) to standard ASCII digits (0-9).
 * 3. Converts Arabic-Indic digits (٠-٩) to standard ASCII digits (0-9).
 */
export function normalizePin(input: string): string {
  if (!input) return "";
  return input
    .trim()
    .replace(/[۰-۹]/g, (char) => String.fromCharCode(char.charCodeAt(0) - 1728))
    .replace(/[٠-٩]/g, (char) => String.fromCharCode(char.charCodeAt(0) - 1584));
}

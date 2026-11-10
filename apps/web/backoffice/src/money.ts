// Amounts are integers of the currency's minor unit; the editor shows and takes them as decimals.

/** `minor` units as a decimal with `digits` after the point: 325 and 2 are "3.25". */
export function format(minor: number, digits: number): string {
  const negative = minor < 0;
  const text = Math.abs(minor).toString().padStart(digits + 1, "0");
  const whole = digits ? text.slice(0, -digits) : text;
  const fraction = digits ? `.${text.slice(-digits)}` : "";
  return `${negative ? "-" : ""}${whole}${fraction}`;
}

/** A decimal typed in, as minor units: `null` if it isn't one, or has too many digits. */
export function parse(text: string, digits: number): number | null {
  const match = /^(-)?(\d+)(?:\.(\d*))?$/.exec(text.trim());
  if (!match) return null;
  const [, sign, whole = "", fraction = ""] = match;
  if (fraction.length > digits) return null;
  const minor = Number(whole + fraction.padEnd(digits, "0"));
  if (!Number.isSafeInteger(minor)) return null;
  return sign ? -minor : minor;
}

/** A new identifier, a UUID of version 7, as the kernel's identifiers are. */
export function newId(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  let millis = Date.now();
  for (let index = 5; index >= 0; index--) {
    bytes[index] = millis % 256;
    millis = Math.floor(millis / 256);
  }
  bytes[6] = 0x70 | ((bytes[6] ?? 0) & 0x0f);
  bytes[8] = 0x80 | ((bytes[8] ?? 0) & 0x3f);
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

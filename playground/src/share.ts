import LZString from "lz-string";

const HASH_PREFIX = "#code/";

const MAX_PAYLOAD_LEN = 100_000;

function encodeSource(code: string): string {
  return LZString.compressToEncodedURIComponent(code);
}

function decodeSource(payload: string): string | null {
  const decoded = LZString.decompressFromEncodedURIComponent(payload);
  return decoded || null;
}

export function readSourceFromHash(): string | null {
  const hash = window.location.hash;
  if (!hash.startsWith(HASH_PREFIX)) return null;
  const payload = hash.slice(HASH_PREFIX.length);
  if (payload.length > MAX_PAYLOAD_LEN) return null;
  return decodeSource(payload);
}

export async function copyShareUrl(code: string): Promise<boolean> {
  const url = `${location.origin}${location.pathname}${location.search}${HASH_PREFIX}${encodeSource(code)}`;
  try {
    await navigator.clipboard.writeText(url);
    return true;
  } catch {
    return false;
  }
}

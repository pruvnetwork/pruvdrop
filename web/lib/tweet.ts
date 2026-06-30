// Shared tweet validation + metadata (server-side; syndication blocks browser CORS).

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function base58Len(s: string): number {
  const bytes = [0];
  for (const ch of s) {
    const val = B58.indexOf(ch);
    if (val < 0) throw new Error("bad b58");
    let carry = val;
    for (let j = 0; j < bytes.length; j++) { carry += bytes[j] * 58; bytes[j] = carry & 0xff; carry = Math.floor(carry / 256); }
    while (carry > 0) { bytes.push(carry & 0xff); carry = Math.floor(carry / 256); }
  }
  for (let k = 0; k < s.length && s[k] === "1"; k++) bytes.push(0);
  return bytes.length;
}
export function extractSolanaAddress(text: string): string | null {
  const cands = text.match(/[1-9A-HJ-NP-Za-km-z]{32,44}/g) || [];
  for (const c of cands) { try { if (base58Len(c) === 32) return c; } catch {} }
  return null;
}
export function parseTweetId(s: string): string | null {
  const m = s.match(/status\/(\d{5,25})/) || s.match(/^(\d{5,25})$/);
  return m ? m[1] : null;
}
function token(id: string): string {
  return ((Number(id) / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, "");
}
const UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

async function fetchRaw(id: string): Promise<any | null> {
  try {
    const r = await fetch(
      `https://cdn.syndication.twimg.com/tweet-result?id=${id}&token=${token(id)}&lang=en`,
      { headers: { "User-Agent": UA, accept: "application/json" }, next: { revalidate: 120 } } as any
    );
    if (!r.ok) return null;
    return await r.json();
  } catch {
    return null;
  }
}

export interface Verdict {
  ok: boolean;
  reason: "ok" | "bad_url" | "not_found" | "no_ticker" | "no_wallet";
  id?: string; handle?: string; likes?: number; wallet?: string;
}

export async function validateTweet(ref: string, ticker: string): Promise<Verdict> {
  const id = parseTweetId(ref);
  if (!id) return { ok: false, reason: "bad_url" };
  const t = await fetchRaw(id);
  if (!t) return { ok: false, reason: "not_found" };

  const text: string = t.text || t.full_text || "";
  const handle: string = (t.user && t.user.screen_name) || "";
  const likes: number = t.favorite_count || 0;
  if (ticker && !text.toLowerCase().includes(ticker.toLowerCase())) return { ok: false, reason: "no_ticker", handle, likes };
  const wallet = extractSolanaAddress(text);
  if (!wallet) return { ok: false, reason: "no_wallet", handle, likes };
  return { ok: true, reason: "ok", id, handle, likes, wallet };
}

/** Current engagement for a tweet (for the live leaderboard). */
export async function fetchTweetMeta(id: string): Promise<{ handle: string; likes: number } | null> {
  const t = await fetchRaw(id);
  if (!t) return null;
  return { handle: (t.user && t.user.screen_name) || "", likes: t.favorite_count || 0 };
}

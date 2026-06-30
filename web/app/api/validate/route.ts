/**
 * Tweet validator (Next.js route handler). The syndication endpoint blocks
 * browser CORS, so we read the tweet server-side and return an instant verdict.
 *
 * GET /api/validate?id=<tweetUrlOrId>&ticker=<$TICKER>
 * -> { ok, reason, handle, likes, wallet }
 *
 * UX only — run-campaign-x re-validates every tweet at ingest.
 */

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
function extractSolanaAddress(text: string): string | null {
  const cands = text.match(/[1-9A-HJ-NP-Za-km-z]{32,44}/g) || [];
  for (const c of cands) { try { if (base58Len(c) === 32) return c; } catch {} }
  return null;
}
function parseTweetId(s: string): string | null {
  const m = s.match(/status\/(\d{5,25})/) || s.match(/^(\d{5,25})$/);
  return m ? m[1] : null;
}
function token(id: string): string {
  return ((Number(id) / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, "");
}
const UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

export async function GET(req: Request) {
  const { searchParams } = new URL(req.url);
  const ref = searchParams.get("id") || "";
  const ticker = (searchParams.get("ticker") || "").toLowerCase();
  const id = parseTweetId(ref);
  if (!id) return Response.json({ ok: false, reason: "bad_url" });

  let t: any;
  try {
    const r = await fetch(`https://cdn.syndication.twimg.com/tweet-result?id=${id}&token=${token(id)}&lang=en`,
      { headers: { "User-Agent": UA, accept: "application/json" } });
    if (!r.ok) return Response.json({ ok: false, reason: "not_found" });
    t = await r.json();
  } catch { return Response.json({ ok: false, reason: "not_found" }); }

  const text: string = t.text || t.full_text || "";
  const handle: string = (t.user && t.user.screen_name) || "";
  const likes: number = t.favorite_count || 0;
  if (ticker && !text.toLowerCase().includes(ticker)) {
    return Response.json({ ok: false, reason: "no_ticker", handle, likes });
  }
  const wallet = extractSolanaAddress(text);
  if (!wallet) return Response.json({ ok: false, reason: "no_wallet", handle, likes });
  return Response.json({ ok: true, reason: "ok", id, handle, likes, wallet });
}

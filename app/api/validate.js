/**
 * Serverless validator for the tweet-submit form (Vercel function).
 *
 * The syndication endpoint only allows CORS from platform.twitter.com, so the
 * browser cannot read tweets directly. This function fetches the tweet
 * server-side and returns an instant verdict to the form:
 *   ticker present? + a valid Solana address embedded? + engagement.
 *
 * GET /api/validate?id=<tweetUrlOrId>&ticker=<$TICKER>
 * -> { ok, reason, handle, likes, wallet }
 *
 * This is UX only — `run-campaign-x` re-validates every tweet authoritatively
 * at ingest time, so the form cannot be used to inject bad entries.
 */

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function base58Decode(s) {
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
function extractSolanaAddress(text) {
  const cands = text.match(/[1-9A-HJ-NP-Za-km-z]{32,44}/g) || [];
  for (const c of cands) { try { if (base58Decode(c) === 32) return c; } catch {} }
  return null;
}
function parseTweetId(s) {
  const m = String(s).match(/status\/(\d{5,25})/) || String(s).match(/^(\d{5,25})$/);
  if (!m) return null;
  return m[1];
}
function token(id) {
  return ((Number(id) / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, "");
}
const UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

export default async function handler(req, res) {
  res.setHeader("Access-Control-Allow-Origin", "*");
  const ref = req.query.id;
  const ticker = String(req.query.ticker || "").toLowerCase();
  const id = parseTweetId(ref);
  if (!id) return res.status(200).json({ ok: false, reason: "bad_url" });

  let t;
  try {
    const r = await fetch(`https://cdn.syndication.twimg.com/tweet-result?id=${id}&token=${token(id)}&lang=en`,
      { headers: { "User-Agent": UA, accept: "application/json" } });
    if (!r.ok) return res.status(200).json({ ok: false, reason: "not_found" });
    t = await r.json();
  } catch { return res.status(200).json({ ok: false, reason: "not_found" }); }

  const text = t.text || t.full_text || "";
  const handle = (t.user && t.user.screen_name) || "";
  const likes = t.favorite_count || 0;
  if (ticker && !text.toLowerCase().includes(ticker)) {
    return res.status(200).json({ ok: false, reason: "no_ticker", handle, likes });
  }
  const wallet = extractSolanaAddress(text);
  if (!wallet) {
    return res.status(200).json({ ok: false, reason: "no_wallet", handle, likes });
  }
  return res.status(200).json({ ok: true, reason: "ok", id, handle, likes, wallet });
}

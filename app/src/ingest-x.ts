/**
 * Layer 1 (X / Twitter) — claim-based, wallet-in-tweet ingestion.
 *
 * No paid X API. Casters tweet about the ticker AND include their Solana
 * address in the tweet text; they (or anyone) submit the tweet URL/ID. We read
 * each tweet via the free syndication endpoint and extract:
 *   - ticker present?   (eligibility)
 *   - engagement        (virality score)
 *   - Solana address    (the recipient wallet — author-authored, so the binding
 *                        is trustless: only the tweet author can put text in it)
 *
 * Tweets are mapped to the same `RawCast` shape used by the Farcaster path, so
 * the rest of the pipeline (score, snapshot, allocate, claim tree, portal) and
 * the verify-to-qualify grace window all work unchanged. A tweet with the
 * ticker + engagement but NO valid Solana address becomes `pending`
 * ("edit/repost including your Solana address to qualify").
 *
 * Caveat: the syndication endpoint is unofficial and engagement fields are
 * limited (likes are reliable; retweet/reply counts may be absent → 0).
 */

import type { RawCast } from "./types.js";
import { base58Decode } from "./claim-tree.js";

/** Extract the numeric tweet id from a URL or a raw id. */
export function parseTweetId(urlOrId: string): string {
  const m = urlOrId.match(/status\/(\d{5,25})/);
  if (m) return m[1];
  const d = urlOrId.trim().match(/^(\d{5,25})$/);
  if (d) return d[1];
  throw new Error(`cannot parse tweet id from: ${urlOrId}`);
}

/** Token the syndication endpoint expects (derived from the tweet id). */
export function syndicationToken(id: string): string {
  return ((Number(id) / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, "");
}

function fnv1a(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 0x01000193); }
  return h >>> 0;
}
/** Deterministic numeric author id for X handles (used where fid is expected). */
export function handleHash(handle: string): number {
  return fnv1a(handle.toLowerCase());
}

/** First substring that is a valid 32-byte base58 (Solana) address, or null. */
export function extractSolanaAddress(text: string): string | null {
  const cands = text.match(/[1-9A-HJ-NP-Za-km-z]{32,44}/g) ?? [];
  for (const c of cands) {
    try { if (base58Decode(c).length === 32) return c; } catch { /* not base58 */ }
  }
  return null;
}

function toUnix(s: unknown): number {
  if (typeof s === "string") { const ms = Date.parse(s); if (!Number.isNaN(ms)) return Math.floor(ms / 1000); }
  return 0;
}

const UA =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

async function fetchTweet(id: string): Promise<any | null> {
  const url = `https://cdn.syndication.twimg.com/tweet-result?id=${id}&token=${syndicationToken(id)}&lang=en`;
  try {
    const res = await fetch(url, { headers: { "User-Agent": UA, accept: "application/json" } });
    if (!res.ok) return null;
    return await res.json();
  } catch { return null; }
}

export interface XIngestOptions {
  ticker: string;        // must appear in the tweet text (case-insensitive)
  windowStart?: number;  // unix seconds; tweets outside are skipped (0 = no bound)
  windowEnd?: number;
}

/**
 * Read submitted tweets and map them to RawCast. `solWallet` is the Solana
 * address embedded in the tweet (null -> pending / verify-to-qualify).
 */
export async function ingestXTweets(idsOrUrls: string[], opts: XIngestOptions): Promise<RawCast[]> {
  const tk = opts.ticker.toLowerCase();
  const ws = opts.windowStart ?? 0;
  const we = opts.windowEnd ?? Number.MAX_SAFE_INTEGER;
  const seen = new Set<string>();
  const out: RawCast[] = [];

  for (const ref of idsOrUrls) {
    let id: string;
    try { id = parseTweetId(ref); } catch { continue; }
    if (seen.has(id)) continue; // dedupe duplicate submissions
    seen.add(id);

    const t = await fetchTweet(id);
    if (!t) continue;
    const text: string = t.text ?? t.full_text ?? "";
    if (tk && !text.toLowerCase().includes(tk)) continue; // ticker required
    const ts = toUnix(t.created_at);
    if (ts && (ts < ws || ts > we)) continue;

    const handle: string = t.user?.screen_name ?? t.user?.screen_name ?? "";
    out.push({
      hash: id,
      fid: handleHash(handle),
      username: handle,
      timestamp: ts,
      followerCount: t.user?.followers_count ?? 0,
      qualityScore: null, // X has no Neynar-style score; set minQualityScore: 0
      solWallet: extractSolanaAddress(text),
      likes: t.favorite_count ?? t.favoriteCount ?? 0,
      recasts: t.retweet_count ?? t.retweetCount ?? 0,
      replies: t.conversation_count ?? t.reply_count ?? 0,
      text,
    });
  }
  return out;
}

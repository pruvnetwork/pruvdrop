/**
 * Layer 1 — Farcaster ingestion via Neynar.
 *
 * Pulls casts containing the campaign query within the time window, with
 * engagement counts, author follower count, Neynar quality score, and the
 * author's verified Solana address (so wallet mapping is free).
 *
 * No paid X API. Uses Node's global fetch (Node >= 18).
 */

import type { CampaignConfig, RawCast } from "./types.js";

const NEYNAR_BASE = "https://api.neynar.com/v2/farcaster";

function apiKey(): string {
  const k = process.env.NEYNAR_API_KEY;
  if (!k) throw new Error("NEYNAR_API_KEY is not set. Get one at https://neynar.com");
  return k;
}

/** Defensive extractor — Neynar field names have shifted across versions. */
function pickNumber(...vals: unknown[]): number {
  for (const v of vals) if (typeof v === "number" && Number.isFinite(v)) return v;
  return 0;
}

function extractSolWallet(author: any): string | null {
  const v =
    author?.verified_addresses?.sol_addresses ??
    author?.verifiedAddresses?.solAddresses ??
    [];
  if (Array.isArray(v) && v.length > 0 && typeof v[0] === "string") return v[0];
  return null;
}

function extractQuality(author: any): number | null {
  const q =
    author?.score ??
    author?.experimental?.neynar_user_score ??
    author?.experimental?.neynarUserScore ??
    null;
  return typeof q === "number" ? q : null;
}

function toUnix(ts: unknown): number {
  if (typeof ts === "number") return ts > 1e12 ? Math.floor(ts / 1000) : ts;
  if (typeof ts === "string") {
    const ms = Date.parse(ts);
    if (!Number.isNaN(ms)) return Math.floor(ms / 1000);
  }
  return 0;
}

function mapCast(c: any): RawCast | null {
  const author = c?.author ?? {};
  const fid = pickNumber(author?.fid);
  if (!fid) return null;
  return {
    hash: String(c?.hash ?? ""),
    fid,
    username: String(author?.username ?? ""),
    timestamp: toUnix(c?.timestamp),
    followerCount: pickNumber(author?.follower_count, author?.followerCount),
    qualityScore: extractQuality(author),
    solWallet: extractSolWallet(author),
    likes: pickNumber(c?.reactions?.likes_count, c?.reactions?.likesCount, c?.reactions?.likes?.length),
    recasts: pickNumber(c?.reactions?.recasts_count, c?.reactions?.recastsCount, c?.reactions?.recasts?.length),
    replies: pickNumber(c?.replies?.count, c?.repliesCount),
    text: String(c?.text ?? ""),
  };
}

/**
 * Fetch all qualifying casts for the campaign (paginated).
 * Stops at windowStart (results are newest-first) or maxCasts.
 */
export async function ingestCasts(cfg: CampaignConfig): Promise<RawCast[]> {
  const key = apiKey();
  const out: RawCast[] = [];
  let cursor: string | undefined = undefined;
  let pages = 0;

  while (out.length < cfg.maxCasts && pages < 200) {
    const url = new URL(`${NEYNAR_BASE}/cast/search`);
    url.searchParams.set("q", cfg.query);
    url.searchParams.set("limit", "100");
    if (cursor) url.searchParams.set("cursor", cursor);

    const res = await fetch(url, {
      headers: { "x-api-key": key, accept: "application/json" },
    });
    if (!res.ok) {
      throw new Error(`Neynar search failed: ${res.status} ${await res.text()}`);
    }
    const data: any = await res.json();
    const casts: any[] = data?.result?.casts ?? data?.casts ?? [];
    if (casts.length === 0) break;

    let reachedOld = false;
    for (const raw of casts) {
      const c = mapCast(raw);
      if (!c) continue;
      if (c.timestamp && c.timestamp < cfg.windowStart) {
        reachedOld = true; // newest-first → everything after is older
        continue;
      }
      if (c.timestamp && c.timestamp > cfg.windowEnd) continue;
      out.push(c);
      if (out.length >= cfg.maxCasts) break;
    }

    cursor = data?.result?.next?.cursor ?? data?.next?.cursor ?? undefined;
    pages++;
    if (!cursor || reachedOld) break;
  }

  return out;
}

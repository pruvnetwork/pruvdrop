/**
 * Layer 1 (cont.) — virality scoring + sybil/quality filtering.
 *
 * Aggregates qualifying casts per author into a single Candidate with a
 * dampened virality score. Eligibility gates run first; whales and swarms
 * are limited by reach-normalization and an optional per-author cap.
 */

import type { CampaignConfig, Candidate, RawCast } from "./types.js";

function castEngagementScore(c: RawCast, cfg: CampaignConfig): number {
  const raw =
    cfg.weights.like * c.likes +
    cfg.weights.recast * c.recasts +
    cfg.weights.reply * c.replies;
  if (cfg.reachExponent <= 0) return raw;
  const denom = Math.pow(Math.max(c.followerCount, 1), cfg.reachExponent);
  return raw / denom;
}

function passesGates(c: RawCast, cfg: CampaignConfig): boolean {
  if (!c.solWallet) return false; // must have a verified Solana address to receive
  if (c.followerCount < cfg.minFollowers) return false;
  if (cfg.minQualityScore > 0) {
    // If quality is unknown, fail closed when a threshold is set.
    if (c.qualityScore === null) return false;
    if (c.qualityScore < cfg.minQualityScore) return false;
  }
  return true;
}

export interface ScoreResult {
  candidates: Candidate[];
  stats: {
    rawCasts: number;
    afterGates: number;
    uniqueAuthors: number;
    droppedNoWallet: number;
    droppedQuality: number;
    droppedFollowers: number;
  };
}

export function scoreAndAggregate(casts: RawCast[], cfg: CampaignConfig): ScoreResult {
  const stats = {
    rawCasts: casts.length,
    afterGates: 0,
    uniqueAuthors: 0,
    droppedNoWallet: 0,
    droppedQuality: 0,
    droppedFollowers: 0,
  };

  const byFid = new Map<number, Candidate>();

  for (const c of casts) {
    if (!c.solWallet) { stats.droppedNoWallet++; continue; }
    if (c.followerCount < cfg.minFollowers) { stats.droppedFollowers++; continue; }
    if (cfg.minQualityScore > 0 && (c.qualityScore === null || c.qualityScore < cfg.minQualityScore)) {
      stats.droppedQuality++; continue;
    }
    if (!passesGates(c, cfg)) continue;

    stats.afterGates++;
    const inc = castEngagementScore(c, cfg);
    const existing = byFid.get(c.fid);
    if (existing) {
      existing.score += inc;
      existing.castCount += 1;
      existing.totalLikes += c.likes;
      existing.totalRecasts += c.recasts;
      existing.totalReplies += c.replies;
    } else {
      byFid.set(c.fid, {
        fid: c.fid,
        username: c.username,
        wallet: c.solWallet!,
        score: inc,
        castCount: 1,
        totalLikes: c.likes,
        totalRecasts: c.recasts,
        totalReplies: c.replies,
        qualityScore: c.qualityScore,
      });
    }
  }

  let candidates = [...byFid.values()];

  // Per-author cap (anti-whale): clamp score to the cap if set.
  if (cfg.perAuthorCap > 0) {
    for (const cand of candidates) cand.score = Math.min(cand.score, cfg.perAuthorCap);
  }

  // Drop zero-score entries.
  candidates = candidates.filter((c) => c.score > 0);

  stats.uniqueAuthors = candidates.length;
  return { candidates, stats };
}

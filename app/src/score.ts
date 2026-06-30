/**
 * Layer 1 (cont.) — virality scoring + sybil/quality filtering.
 *
 * Aggregates qualifying casts per author into a single Candidate with a
 * dampened virality score. Eligibility gates run first; whales and swarms
 * are limited by reach-normalization and an optional per-author cap.
 *
 * Verify-to-qualify grace window: casters who pass the quality + follower
 * gates but have NO Solana verified address are NOT discarded — they are
 * collected as `pending` so the campaign can tell them "verify your Solana
 * wallet on Farcaster to qualify" before the snapshot is finalized.
 */

import type { CampaignConfig, Candidate, RawCast } from "./types.js";

/** Otherwise-eligible caster who lacks a Solana verified address. */
export interface PendingCaster {
  fid: number;
  username: string;
  score: number;
  castCount: number;
  totalLikes: number;
  totalRecasts: number;
  totalReplies: number;
  qualityScore: number | null;
}

function castEngagementScore(c: RawCast, cfg: CampaignConfig): number {
  const raw =
    cfg.weights.like * c.likes +
    cfg.weights.recast * c.recasts +
    cfg.weights.reply * c.replies;
  if (cfg.reachExponent <= 0) return raw;
  const denom = Math.pow(Math.max(c.followerCount, 1), cfg.reachExponent);
  return raw / denom;
}

export interface ScoreResult {
  candidates: Candidate[];
  /** Pass quality + followers but missing a Solana wallet — verify to qualify. */
  pending: PendingCaster[];
  stats: {
    rawCasts: number;
    afterGates: number;
    uniqueAuthors: number;
    pendingAuthors: number;
    droppedQuality: number;
    droppedFollowers: number;
  };
}

export function scoreAndAggregate(casts: RawCast[], cfg: CampaignConfig): ScoreResult {
  const stats = {
    rawCasts: casts.length,
    afterGates: 0,
    uniqueAuthors: 0,
    pendingAuthors: 0,
    droppedQuality: 0,
    droppedFollowers: 0,
  };

  const byFid = new Map<number, Candidate>();
  const pendingByFid = new Map<number, PendingCaster>();

  for (const c of casts) {
    // Real eligibility gates first (these are genuine disqualifications).
    if (c.followerCount < cfg.minFollowers) { stats.droppedFollowers++; continue; }
    if (cfg.minQualityScore > 0 && (c.qualityScore === null || c.qualityScore < cfg.minQualityScore)) {
      stats.droppedQuality++; continue;
    }

    stats.afterGates++;
    const inc = castEngagementScore(c, cfg);

    if (!c.solWallet) {
      // Verify-to-qualify: otherwise eligible, just no Solana address yet.
      const ex = pendingByFid.get(c.fid);
      if (ex) {
        ex.score += inc; ex.castCount += 1;
        ex.totalLikes += c.likes; ex.totalRecasts += c.recasts; ex.totalReplies += c.replies;
      } else {
        pendingByFid.set(c.fid, {
          fid: c.fid, username: c.username, score: inc, castCount: 1,
          totalLikes: c.likes, totalRecasts: c.recasts, totalReplies: c.replies,
          qualityScore: c.qualityScore,
        });
      }
      continue;
    }

    // Eligible candidate (has a Solana verified address).
    const existing = byFid.get(c.fid);
    if (existing) {
      existing.score += inc; existing.castCount += 1;
      existing.totalLikes += c.likes; existing.totalRecasts += c.recasts; existing.totalReplies += c.replies;
    } else {
      byFid.set(c.fid, {
        fid: c.fid, username: c.username, wallet: c.solWallet, score: inc, castCount: 1,
        totalLikes: c.likes, totalRecasts: c.recasts, totalReplies: c.replies, qualityScore: c.qualityScore,
      });
    }
  }

  let candidates = [...byFid.values()];
  let pending = [...pendingByFid.values()];

  // Per-author cap (anti-whale): clamp score to the cap if set.
  if (cfg.perAuthorCap > 0) {
    for (const x of candidates) x.score = Math.min(x.score, cfg.perAuthorCap);
    for (const x of pending) x.score = Math.min(x.score, cfg.perAuthorCap);
  }

  candidates = candidates.filter((c) => c.score > 0);
  pending = pending.filter((c) => c.score > 0).sort((a, b) => b.score - a.score);

  stats.uniqueAuthors = candidates.length;
  stats.pendingAuthors = pending.length;
  return { candidates, pending, stats };
}

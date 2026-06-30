/**
 * Viral Airdrop — shared types.
 *
 * Layer 1 (ingest) + Layer 2 (snapshot/commit) produce a committed candidate
 * set that PRUV's verifiable allocation (Layer 3) consumes.
 */

/** A single qualifying cast pulled from Farcaster. */
export interface RawCast {
  hash: string;
  fid: number;
  username: string;
  timestamp: number; // unix seconds
  followerCount: number;
  /** Neynar user-quality score in [0,1] if available, else null. */
  qualityScore: number | null;
  /** First verified Solana address on the author profile, or null. */
  solWallet: string | null;
  likes: number;
  recasts: number;
  replies: number;
  text: string;
}

/** One airdrop candidate (aggregated per author). */
export interface Candidate {
  fid: number;
  username: string;
  wallet: string; // base58 Solana address
  /** Final virality score (post-aggregation, post-dampening). */
  score: number;
  /** Diagnostics. */
  castCount: number;
  totalLikes: number;
  totalRecasts: number;
  totalReplies: number;
  qualityScore: number | null;
}

export interface ScoringWeights {
  like: number;
  recast: number;
  reply: number;
}

export interface CampaignConfig {
  /** Search query — usually the ticker/cashtag, e.g. "$PRUV" or "pruv". */
  query: string;
  /** Campaign window (unix seconds). Casts outside are ignored. */
  windowStart: number;
  windowEnd: number;
  /** Eligibility gates. */
  minQualityScore: number; // 0 disables; Neynar score in [0,1]
  minFollowers: number;
  /** Scoring. */
  weights: ScoringWeights;
  /** Reach normalization: divide raw engagement by followers^reachExponent.
   *  0 = no normalization (raw engagement). 0.5 = sqrt(followers) dampening. */
  reachExponent: number;
  /** Per-author score cap (0 = no cap) so whales cannot dominate. */
  perAuthorCap: number;
  /** Ingestion safety cap. */
  maxCasts: number;
}

export const DEFAULT_CONFIG: Omit<CampaignConfig, "query" | "windowStart" | "windowEnd"> = {
  minQualityScore: 0.5,
  minFollowers: 0,
  weights: { like: 1, recast: 3, reply: 2 },
  reachExponent: 0.3,
  perAuthorCap: 0,
  maxCasts: 5000,
};

/** Canonical, committed snapshot handed to PRUV allocation (Layer 3). */
export interface Snapshot {
  campaign: CampaignConfig;
  builtAtSlotHint: string; // human note; real seed comes from the on-chain commit
  candidates: Candidate[]; // canonical order: ascending fid
  /** Hex SHA-256 Merkle root of the candidate set. Commit THIS on-chain
   *  before the allocation seed is known (PRUV commit-before-seed). */
  merkleRoot: string;
  leafCount: number;
}

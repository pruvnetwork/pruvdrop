/**
 * Layer 3 — PRUV verifiable allocation.
 *
 * Two modes, both recomputable by anyone from the committed snapshot:
 *
 *   top-n            deterministic ranking by score (no seed needed)
 *   weighted-lottery N distinct winners drawn with probability ∝ score,
 *                    using the on-chain slot-hash as the verifiable seed
 *                    (same seed source PRUV's lottery uses: fetchSlotHashForSlot)
 *
 * The seed is unknown at snapshot-commit time (commit-before-seed), so the
 * operator cannot steer the outcome. Given (snapshot, seed) the result is a
 * pure function — see `verifyAllocation`.
 *
 * NOTE: the draw construction below is canonical. An on-chain/Rust verifier
 * must mirror it byte-for-byte (exactly as `deriveWinnerIndex` mirrors its
 * Rust counterpart).
 */

import { createHash } from "node:crypto";
import type { Candidate, Snapshot } from "./types.js";
import { scaleScore } from "./snapshot.js";

export type AllocationMode = "top-n" | "weighted-lottery";
export type PayoutMode = "equal" | "proportional";

export interface AllocationConfig {
  mode: AllocationMode;
  /** Number of winners. */
  winners: number;
  /** Total token amount to distribute, in base units. */
  totalAmount: bigint;
  /** How winners split the pot. */
  payout: PayoutMode;
}

export interface Award {
  fid: number;
  username: string;
  wallet: string;
  amount: string; // base units, as string (JSON-safe)
  score: number;
  rank?: number; // top-n only
  drawOrder?: number; // lottery only
}

export interface AllocationResult {
  mode: AllocationMode;
  payout: PayoutMode;
  snapshotRoot: string;
  seedHex: string | null; // null for top-n
  winners: number;
  totalAmount: string;
  totalDistributed: string;
  awards: Award[];
}

// ── deterministic helpers ──────────────────────────────────────────────────

function sha256(buf: Buffer): Buffer {
  return createHash("sha256").update(buf).digest();
}

/** u64 (LE) drawn from sha256(seed || counterLE) — canonical lottery draw. */
function drawU64(seed: Buffer, counter: number): bigint {
  const cb = Buffer.alloc(4);
  cb.writeUInt32LE(counter >>> 0);
  const h = sha256(Buffer.concat([seed, cb]));
  return h.readBigUInt64LE(0);
}

/** Distribute `total` across `n` weighted slots, integer base units, no dust lost. */
function splitAmounts(total: bigint, weights: bigint[]): bigint[] {
  const n = weights.length;
  if (n === 0) return [];
  const sumW = weights.reduce((a, b) => a + b, 0n);
  if (sumW === 0n) {
    // equal split fallback
    const base = total / BigInt(n);
    const out = new Array<bigint>(n).fill(base);
    let rem = total - base * BigInt(n);
    for (let i = 0; i < n && rem > 0n; i++, rem--) out[i] += 1n;
    return out;
  }
  const out = weights.map((w) => (total * w) / sumW);
  let distributed = out.reduce((a, b) => a + b, 0n);
  let rem = total - distributed;
  // Hand remainder to the highest weights first (deterministic).
  const order = weights.map((w, i) => [w, i] as const).sort((a, b) => (b[0] > a[0] ? 1 : b[0] < a[0] ? -1 : a[1] - b[1]));
  for (let k = 0; k < order.length && rem > 0n; k++, rem--) out[order[k][1]] += 1n;
  return out;
}

// ── mode: top-n ─────────────────────────────────────────────────────────────

export function allocateTopN(snapshot: Snapshot, cfg: AllocationConfig): AllocationResult {
  // Deterministic ranking: score desc, then fid asc (stable tiebreak).
  const ranked = [...snapshot.candidates].sort((a, b) =>
    b.score !== a.score ? b.score - a.score : a.fid - b.fid
  );
  const winners = ranked.slice(0, Math.min(cfg.winners, ranked.length));

  const weights =
    cfg.payout === "proportional"
      ? winners.map((c) => scaleScore(c.score))
      : winners.map(() => 1n);
  const amounts = splitAmounts(cfg.totalAmount, weights);

  const awards: Award[] = winners.map((c, i) => ({
    fid: c.fid,
    username: c.username,
    wallet: c.wallet,
    amount: amounts[i].toString(),
    score: c.score,
    rank: i + 1,
  }));

  return {
    mode: "top-n",
    payout: cfg.payout,
    snapshotRoot: snapshot.merkleRoot,
    seedHex: null,
    winners: awards.length,
    totalAmount: cfg.totalAmount.toString(),
    totalDistributed: amounts.reduce((a, b) => a + b, 0n).toString(),
    awards,
  };
}

// ── mode: weighted-lottery ───────────────────────────────────────────────────

export function allocateWeightedLottery(
  snapshot: Snapshot,
  cfg: AllocationConfig,
  seed: Buffer
): AllocationResult {
  if (seed.length !== 32) throw new Error(`seed must be 32 bytes, got ${seed.length}`);

  // Active pool with integer weights (scaled scores).
  const pool = snapshot.candidates
    .map((c) => ({ c, w: scaleScore(c.score) }))
    .filter((e) => e.w > 0n);

  const n = Math.min(cfg.winners, pool.length);
  const selected: { c: Candidate; order: number }[] = [];
  let totalW = pool.reduce((a, e) => a + e.w, 0n);

  for (let k = 0; k < n; k++) {
    if (totalW <= 0n) break;
    const target = drawU64(seed, k) % totalW; // in [0, totalW)
    // Walk cumulative weights to find the winner.
    let cum = 0n;
    let pick = -1;
    for (let i = 0; i < pool.length; i++) {
      cum += pool[i].w;
      if (target < cum) { pick = i; break; }
    }
    if (pick < 0) pick = pool.length - 1;
    const [chosen] = pool.splice(pick, 1);
    totalW -= chosen.w;
    selected.push({ c: chosen.c, order: k });
  }

  const weights =
    cfg.payout === "proportional"
      ? selected.map((s) => scaleScore(s.c.score))
      : selected.map(() => 1n);
  const amounts = splitAmounts(cfg.totalAmount, weights);

  const awards: Award[] = selected.map((s, i) => ({
    fid: s.c.fid,
    username: s.c.username,
    wallet: s.c.wallet,
    amount: amounts[i].toString(),
    score: s.c.score,
    drawOrder: s.order,
  }));

  return {
    mode: "weighted-lottery",
    payout: cfg.payout,
    snapshotRoot: snapshot.merkleRoot,
    seedHex: seed.toString("hex"),
    winners: awards.length,
    totalAmount: cfg.totalAmount.toString(),
    totalDistributed: amounts.reduce((a, b) => a + b, 0n).toString(),
    awards,
  };
}

// ── dispatcher + verifier ────────────────────────────────────────────────────

export function allocate(
  snapshot: Snapshot,
  cfg: AllocationConfig,
  seed?: Buffer
): AllocationResult {
  if (cfg.mode === "top-n") return allocateTopN(snapshot, cfg);
  if (!seed) throw new Error("weighted-lottery requires a 32-byte seed (on-chain slot hash)");
  return allocateWeightedLottery(snapshot, cfg, seed);
}

/**
 * Independent re-computation: anyone with the committed snapshot, the config,
 * and the on-chain seed can recompute the exact allocation and compare.
 */
export function verifyAllocation(
  snapshot: Snapshot,
  cfg: AllocationConfig,
  result: AllocationResult,
  seed?: Buffer
): boolean {
  if (snapshot.merkleRoot !== result.snapshotRoot) return false;
  const recomputed = allocate(snapshot, cfg, seed);
  return JSON.stringify(recomputed.awards) === JSON.stringify(result.awards);
}

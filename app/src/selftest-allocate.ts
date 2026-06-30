/**
 * Offline self-test for Layer 3 (allocation). No network needed.
 */
import type { Candidate, CampaignConfig } from "./types.js";
import { DEFAULT_CONFIG } from "./types.js";
import { buildSnapshot } from "./snapshot.js";
import {
  allocateTopN,
  allocateWeightedLottery,
  verifyAllocation,
  type AllocationConfig,
} from "./allocate.js";

function assert(cond: boolean, msg: string) {
  if (!cond) { console.error("FAIL:", msg); process.exit(1); }
}
function sum(strs: string[]): bigint { return strs.reduce((a, s) => a + BigInt(s), 0n); }

const campaign: CampaignConfig = { ...DEFAULT_CONFIG, query: "$T", windowStart: 0, windowEnd: 1 };

function mk(fid: number, score: number): Candidate {
  return { fid, username: `u${fid}`, wallet: `W${fid}`, score,
    castCount: 1, totalLikes: 0, totalRecasts: 0, totalReplies: 0, qualityScore: 0.9 };
}

// 8 candidates with distinct scores.
const cands = [mk(1, 100), mk(2, 80), mk(3, 60), mk(4, 40), mk(5, 30), mk(6, 20), mk(7, 10), mk(8, 5)];
const snap = buildSnapshot(cands, campaign);

const TOTAL = 1_000_000n;

// ── top-N ──────────────────────────────────────────────────────────────────
const topCfg: AllocationConfig = { mode: "top-n", winners: 3, totalAmount: TOTAL, payout: "equal" };
const top = allocateTopN(snap, topCfg);
assert(top.awards.length === 3, "top-n picks 3");
assert(top.awards[0].fid === 1 && top.awards[1].fid === 2 && top.awards[2].fid === 3, "top-n ranked by score desc");
assert(top.awards[0].rank === 1, "top-n rank set");
assert(sum(top.awards.map(a => a.amount)) === TOTAL, "top-n equal payout sums to total (no dust)");
assert(verifyAllocation(snap, topCfg, top), "top-n verifies");

const topProp = allocateTopN(snap, { ...topCfg, payout: "proportional" });
assert(sum(topProp.awards.map(a => a.amount)) === TOTAL, "top-n proportional sums to total");
assert(BigInt(topProp.awards[0].amount) > BigInt(topProp.awards[2].amount), "top-n proportional: higher score gets more");

// ── weighted-lottery ──────────────────────────────────────────────────────
const seedA = Buffer.alloc(32, 7);
const seedB = Buffer.alloc(32, 9);
const lotCfg: AllocationConfig = { mode: "weighted-lottery", winners: 4, totalAmount: TOTAL, payout: "equal" };

const lot1 = allocateWeightedLottery(snap, lotCfg, seedA);
const lot2 = allocateWeightedLottery(snap, lotCfg, seedA);
assert(JSON.stringify(lot1.awards) === JSON.stringify(lot2.awards), "lottery deterministic for same seed");
assert(lot1.awards.length === 4, "lottery picks 4");
const fids = lot1.awards.map(a => a.fid);
assert(new Set(fids).size === fids.length, "lottery winners are distinct (no replacement)");
assert(sum(lot1.awards.map(a => a.amount)) === TOTAL, "lottery payout sums to total");
assert(verifyAllocation(snap, lotCfg, lot1, seedA), "lottery verifies with correct seed");
assert(!verifyAllocation(snap, lotCfg, lot1, seedB), "lottery FAILS verify with wrong seed");

const lotB = allocateWeightedLottery(snap, lotCfg, seedB);
assert(JSON.stringify(lotB.awards) !== JSON.stringify(lot1.awards), "different seed -> different draw");

console.log("PASS — Layer 3 allocation: top-N + weighted-lottery, payouts, determinism, verification all OK");
console.log(`  top-N winners (fids): ${top.awards.map(a=>a.fid).join(", ")}`);
console.log(`  lottery seedA winners (fids): ${lot1.awards.map(a=>a.fid).join(", ")}`);
console.log(`  lottery seedB winners (fids): ${lotB.awards.map(a=>a.fid).join(", ")}`);

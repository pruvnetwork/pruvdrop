/**
 * Offline self-test — no Neynar key needed.
 * Mocks casts, runs scoring + snapshot, and checks determinism + Merkle root.
 */
import type { CampaignConfig, RawCast } from "./types.js";
import { DEFAULT_CONFIG } from "./types.js";
import { scoreAndAggregate } from "./score.js";
import { buildSnapshot, merkleRoot, leafHash } from "./snapshot.js";

function assert(cond: boolean, msg: string) {
  if (!cond) { console.error("FAIL:", msg); process.exit(1); }
}

const cfg: CampaignConfig = {
  ...DEFAULT_CONFIG,
  query: "$TEST",
  windowStart: 1000,
  windowEnd: 2000,
  minQualityScore: 0.5,
  minFollowers: 10,
};

function cast(p: Partial<RawCast>): RawCast {
  return {
    hash: "h", fid: 1, username: "u", timestamp: 1500, followerCount: 100,
    qualityScore: 0.9, solWallet: "SoLwallet1111111111111111111111111111111111",
    likes: 0, recasts: 0, replies: 0, text: "$TEST", ...p,
  };
}

const casts: RawCast[] = [
  cast({ fid: 10, username: "alice", solWallet: "AAAA", likes: 100, recasts: 20, replies: 10, followerCount: 500, qualityScore: 0.9 }),
  cast({ fid: 10, username: "alice", solWallet: "AAAA", likes: 50, recasts: 5, replies: 2, followerCount: 500, qualityScore: 0.9 }), // same author, aggregates
  cast({ fid: 20, username: "bob", solWallet: "BBBB", likes: 5, recasts: 1, replies: 0, followerCount: 50, qualityScore: 0.8 }),
  cast({ fid: 30, username: "nowallet", solWallet: null, likes: 999, recasts: 999, replies: 999, followerCount: 9000, qualityScore: 0.99 }), // dropped: no wallet
  cast({ fid: 40, username: "lowq", solWallet: "DDDD", likes: 999, recasts: 999, replies: 999, followerCount: 9000, qualityScore: 0.2 }),  // dropped: quality
  cast({ fid: 50, username: "lowfoll", solWallet: "EEEE", likes: 999, recasts: 999, replies: 999, followerCount: 3, qualityScore: 0.9 }),  // dropped: followers
];

const { candidates, stats } = scoreAndAggregate(casts, cfg);

assert(stats.droppedNoWallet === 1, `no-wallet drop (got ${stats.droppedNoWallet})`);
assert(stats.droppedQuality === 1, `quality drop (got ${stats.droppedQuality})`);
assert(stats.droppedFollowers === 1, `followers drop (got ${stats.droppedFollowers})`);
assert(stats.uniqueAuthors === 2, `unique authors (got ${stats.uniqueAuthors})`);

const alice = candidates.find((c) => c.fid === 10)!;
assert(alice.castCount === 2, "alice aggregated 2 casts");
assert(alice.totalLikes === 150, "alice total likes 150");
assert(alice.score > 0, "alice has score");

// Snapshot determinism: same input -> same root.
const s1 = buildSnapshot(candidates, cfg);
const s2 = buildSnapshot([...candidates].reverse(), cfg); // order should not matter (canonical sort)
assert(s1.merkleRoot === s2.merkleRoot, "snapshot root is order-independent (canonical)");
assert(s1.leafCount === 2, "leaf count 2");
assert(s1.candidates[0].fid < s1.candidates[1].fid, "canonical ascending fid");

// Merkle sanity: single leaf root == leaf.
const oneLeaf = leafHash(candidates[0]);
assert(merkleRoot([oneLeaf]).equals(oneLeaf), "single-leaf root equals leaf");

console.log("PASS — scoring, gates, aggregation, canonical snapshot, Merkle root all OK");
console.log(`  root: ${s1.merkleRoot}`);
console.log(`  alice score=${alice.score.toFixed(4)}  bob score=${candidates.find(c=>c.fid===20)!.score.toFixed(4)}`);

/**
 * Offline self-test for Layer 5 claim tree (TS side).
 * Mirrors the on-chain hashing; verifies every proof and rejects tampering.
 */
import type { Award } from "./allocate.js";
import { buildClaimTree, leafHash, verifyProof } from "./claim-tree.js";

function assert(cond: boolean, msg: string) {
  if (!cond) { console.error("FAIL:", msg); process.exit(1); }
}

// Valid base58 Solana pubkeys (system program, token program, a few known keys).
const wallets = [
  "11111111111111111111111111111112",            // system-ish (32 bytes)
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",  // token program
  "So11111111111111111111111111111111111111112",  // wrapped SOL mint
  "SysvarRent111111111111111111111111111111111",
  "SysvarC1ock11111111111111111111111111111111",
];

const awards: Award[] = wallets.map((w, i) => ({
  fid: 100 + i, username: `u${i}`, wallet: w,
  amount: String(1000 * (i + 1)), score: 10 - i,
}));

const tree = buildClaimTree(awards);
const root = Buffer.from(tree.merkleRoot, "hex");
assert(tree.count === awards.length, "all awards in tree");

// Every claim's proof verifies against the root.
for (const c of tree.claims) {
  const leaf = leafHash(c.index, c.wallet, BigInt(c.amount));
  const proof = c.proof.map((h) => Buffer.from(h, "hex"));
  assert(verifyProof(leaf, proof, root), `proof verifies for index ${c.index}`);
}

// Tampering with the amount breaks the proof.
const c0 = tree.claims[0];
const badLeaf = leafHash(c0.index, c0.wallet, BigInt(c0.amount) + 1n);
const badProof = c0.proof.map((h) => Buffer.from(h, "hex"));
assert(!verifyProof(badLeaf, badProof, root), "tampered amount fails verification");

// Wrong index fails too.
const badIdxLeaf = leafHash(c0.index + 99, c0.wallet, BigInt(c0.amount));
assert(!verifyProof(badIdxLeaf, badProof, root), "wrong index fails verification");

// Determinism.
const tree2 = buildClaimTree(awards);
assert(tree2.merkleRoot === tree.merkleRoot, "claim root deterministic");

console.log("PASS — Layer 5 claim tree: proofs verify, tampering rejected, deterministic");
console.log(`  claim root: ${tree.merkleRoot}`);
console.log(`  proof length (depth) for index 0: ${tree.claims[0].proof.length}`);

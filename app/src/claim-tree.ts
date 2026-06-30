/**
 * Layer 5 (TS side) — claim Merkle tree.
 *
 * Separate from the snapshot tree. Leaves are keyed on (index, wallet, amount)
 * so the on-chain claim program can verify a recipient's award. Hashing MUST
 * match the Rust program in `programs/viral-airdrop-claim`:
 *
 *   leaf = sha256( 0x00 || u64LE(index) || pubkey[32] || u64LE(amount) )
 *   node = sha256( 0x01 || min(a,b) || max(a,b) )      // sorted pair
 *
 * Sorted-pair node hashing makes proofs direction-independent (no left/right
 * flags), and the 0x00/0x01 domain tags prevent leaf/node confusion.
 */

import { createHash } from "node:crypto";
import type { Award } from "./allocate.js";

const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

export function base58Decode(s: string): Uint8Array {
  const bytes: number[] = [0];
  for (const ch of s) {
    const val = B58.indexOf(ch);
    if (val < 0) throw new Error(`invalid base58 char '${ch}'`);
    let carry = val;
    for (let j = 0; j < bytes.length; j++) {
      carry += bytes[j] * 58;
      bytes[j] = carry & 0xff;
      carry = Math.floor(carry / 256);
    }
    while (carry > 0) { bytes.push(carry & 0xff); carry = Math.floor(carry / 256); }
  }
  for (let k = 0; k < s.length && s[k] === "1"; k++) bytes.push(0);
  return Uint8Array.from(bytes.reverse());
}

function sha256(buf: Buffer): Buffer {
  return createHash("sha256").update(buf).digest();
}

function u64le(v: bigint): Buffer {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(v);
  return b;
}

export function leafHash(index: number, walletB58: string, amount: bigint): Buffer {
  const pk = base58Decode(walletB58);
  if (pk.length !== 32) throw new Error(`wallet ${walletB58} did not decode to 32 bytes (${pk.length})`);
  return sha256(Buffer.concat([Buffer.from([0x00]), u64le(BigInt(index)), Buffer.from(pk), u64le(amount)]));
}

export function nodeHash(a: Buffer, b: Buffer): Buffer {
  const [lo, hi] = Buffer.compare(a, b) <= 0 ? [a, b] : [b, a];
  return sha256(Buffer.concat([Buffer.from([0x01]), lo, hi]));
}

function buildLayers(leaves: Buffer[]): Buffer[][] {
  if (leaves.length === 0) return [[Buffer.alloc(32)]];
  const layers: Buffer[][] = [leaves];
  while (layers[layers.length - 1].length > 1) {
    const cur = layers[layers.length - 1];
    const next: Buffer[] = [];
    for (let i = 0; i < cur.length; i += 2) {
      const l = cur[i];
      const r = i + 1 < cur.length ? cur[i + 1] : cur[i]; // duplicate last
      next.push(nodeHash(l, r));
    }
    layers.push(next);
  }
  return layers;
}

function proofFor(layers: Buffer[][], index: number): Buffer[] {
  const proof: Buffer[] = [];
  let idx = index;
  for (let l = 0; l < layers.length - 1; l++) {
    const layer = layers[l];
    const sibIdx = idx ^ 1;
    proof.push(sibIdx < layer.length ? layer[sibIdx] : layer[idx]);
    idx >>= 1;
  }
  return proof;
}

export function verifyProof(leaf: Buffer, proof: Buffer[], root: Buffer): boolean {
  let computed = leaf;
  for (const sib of proof) computed = nodeHash(computed, sib);
  return computed.equals(root);
}

export interface ClaimEntry {
  index: number;
  wallet: string;
  amount: string;
  proof: string[]; // hex sibling hashes
}

export interface ClaimTree {
  merkleRoot: string;
  count: number;
  claims: ClaimEntry[];
}

/** Build the claim tree from allocation awards (index = position in awards). */
export function buildClaimTree(awards: Award[]): ClaimTree {
  const leaves = awards.map((a, i) => leafHash(i, a.wallet, BigInt(a.amount)));
  const layers = buildLayers(leaves);
  const root = layers[layers.length - 1][0];
  const claims: ClaimEntry[] = awards.map((a, i) => ({
    index: i,
    wallet: a.wallet,
    amount: a.amount,
    proof: proofFor(layers, i).map((p) => p.toString("hex")),
  }));
  return { merkleRoot: root.toString("hex"), count: awards.length, claims };
}

/**
 * Layer 2 — canonical snapshot + Merkle commitment.
 *
 * Produces a deterministic, ordered candidate set and a SHA-256 Merkle root.
 * Commit the root on-chain BEFORE the allocation seed is known (PRUV's
 * commit-before-seed property). Publish the snapshot JSON publicly (IPFS /
 * Arweave) so anyone can independently re-pull the open Farcaster data and
 * recompute this root.
 */

import { createHash } from "node:crypto";
import type { Candidate, CampaignConfig, Snapshot } from "./types.js";

function sha256(buf: Buffer): Buffer {
  return createHash("sha256").update(buf).digest();
}

/** Score is scaled to an integer (6 dp) so the leaf is deterministic. */
export function scaleScore(score: number): bigint {
  return BigInt(Math.round(score * 1e6));
}

/** Canonical leaf: sha256("fid:wallet:scaledScore"). */
export function leafHash(c: Candidate): Buffer {
  const canonical = `${c.fid}:${c.wallet}:${scaleScore(c.score).toString()}`;
  return sha256(Buffer.from(canonical, "utf8"));
}

/** Standard binary Merkle root (duplicate last node on odd levels). */
export function merkleRoot(leaves: Buffer[]): Buffer {
  if (leaves.length === 0) return Buffer.alloc(32);
  let level = leaves;
  while (level.length > 1) {
    const next: Buffer[] = [];
    for (let i = 0; i < level.length; i += 2) {
      const left = level[i];
      const right = i + 1 < level.length ? level[i + 1] : level[i]; // duplicate last
      next.push(sha256(Buffer.concat([left, right])));
    }
    level = next;
  }
  return level[0];
}

export function buildSnapshot(candidates: Candidate[], campaign: CampaignConfig): Snapshot {
  // Canonical order: ascending fid (deterministic, operator cannot reorder).
  const ordered = [...candidates].sort((a, b) => a.fid - b.fid);
  const leaves = ordered.map(leafHash);
  const root = merkleRoot(leaves);
  return {
    campaign,
    builtAtSlotHint: "commit this root on-chain before the seed slot",
    candidates: ordered,
    merkleRoot: root.toString("hex"),
    leafCount: ordered.length,
  };
}

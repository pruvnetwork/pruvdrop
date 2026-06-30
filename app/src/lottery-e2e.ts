/**
 * End-to-end verifiable lottery — ties the on-chain commit-before-seed to the
 * off-chain weighted-lottery allocation, seeded by a real on-chain slot hash.
 *
 *   commit(snapshotRoot, seedSlot)   <- on-chain, BEFORE seedSlot exists
 *   wait for seedSlot
 *   seed = slot hash at seedSlot      <- unknowable at commit time
 *   allocateWeightedLottery(snapshot, cfg, seed)
 *
 * The on-chain commit is injected via `commit` so this module stays free of
 * any specific Anchor wiring; the caller (test/CLI) supplies it.
 *
 * Verifiable: anyone reads the committed (snapshot_root, seed_slot) from chain,
 * fetches the slot hash, and recomputes the allocation with `verifyAllocation`.
 */

import { Connection } from "@solana/web3.js";
import type { Snapshot } from "./types.js";
import {
  allocateWeightedLottery,
  verifyAllocation,
  type AllocationConfig,
  type AllocationResult,
} from "./allocate.js";
import { chooseSeedSlot, awaitSeed } from "./commit.js";

/** Hex Merkle root -> byte array for the on-chain instruction arg. */
export function rootToBytes(hexRoot: string): number[] {
  return [...Buffer.from(hexRoot, "hex")];
}

/** Caller-supplied on-chain commit: `commit_snapshot(rootBytes, seedSlot)`. */
export type CommitFn = (rootBytes: number[], seedSlot: number) => Promise<void>;

export async function runVerifiableLottery(opts: {
  connection: Connection;
  snapshot: Snapshot;
  cfg: AllocationConfig;
  commit: CommitFn;
  delaySlots?: number;
}): Promise<{ seedSlot: number; seed: Buffer; result: AllocationResult }> {
  if (opts.cfg.mode !== "weighted-lottery") {
    throw new Error("runVerifiableLottery requires mode 'weighted-lottery'");
  }
  // 1. choose a FUTURE seed slot and commit the snapshot root to it.
  const seedSlot = await chooseSeedSlot(opts.connection, opts.delaySlots ?? 12);
  await opts.commit(rootToBytes(opts.snapshot.merkleRoot), seedSlot);
  // 2. wait for the slot, then read its hash = the verifiable seed.
  const seed = await awaitSeed(opts.connection, seedSlot);
  // 3. allocate.
  const result = allocateWeightedLottery(opts.snapshot, opts.cfg, seed);
  return { seedSlot, seed, result };
}

/** Independent recomputation against the on-chain seed slot. */
export async function verifyVerifiableLottery(opts: {
  connection: Connection;
  snapshot: Snapshot;
  cfg: AllocationConfig;
  seedSlot: number;
  result: AllocationResult;
}): Promise<boolean> {
  const seed = await awaitSeed(opts.connection, opts.seedSlot);
  return verifyAllocation(opts.snapshot, opts.cfg, opts.result, seed);
}

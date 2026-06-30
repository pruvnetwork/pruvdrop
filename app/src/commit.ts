/**
 * Commit-before-seed — off-chain helpers.
 *
 * Flow:
 *   1. build snapshot (Layer 2) -> snapshotRoot
 *   2. pick a FUTURE seed slot, call on-chain `commit_snapshot(snapshotRoot, seedSlot)`
 *      (the seed is unknowable now -> operator cannot steer the lottery)
 *   3. wait until seedSlot passes
 *   4. fetchSeed(seedSlot) -> 32-byte slot hash = the verifiable seed
 *   5. allocateWeightedLottery(snapshot, cfg, seed)
 *
 * Anyone can verify: read (snapshotRoot, seedSlot) from the on-chain commitment,
 * fetch the slot hash, recompute the allocation, and compare.
 *
 * The SlotHashes parser is identical to `../sdk` `fetchSlotHashForSlot`.
 */

import { Connection, PublicKey } from "@solana/web3.js";

export const SYSVAR_SLOT_HASHES = new PublicKey(
  "SysvarS1otHashes111111111111111111111111111"
);

export async function getCurrentSlot(conn: Connection): Promise<number> {
  return conn.getSlot("confirmed");
}

/** Pick a future seed slot. Commit BEFORE this slot is produced. */
export async function chooseSeedSlot(conn: Connection, delaySlots = 50): Promise<number> {
  return (await getCurrentSlot(conn)) + delaySlots;
}

/**
 * Fetch the 32-byte slot hash for `seedSlot` (the verifiable lottery seed).
 * Must be called while `seedSlot` is still within the SlotHashes window
 * (~512 most-recent slots), i.e. shortly after it has passed.
 */
export async function fetchSeed(conn: Connection, seedSlot: number | bigint): Promise<Buffer> {
  const acc = await conn.getAccountInfo(SYSVAR_SLOT_HASHES, "confirmed");
  if (!acc) throw new Error("SlotHashes sysvar not found");
  const data = acc.data;
  const count = Number(data.readBigUInt64LE(0));
  const ENTRY = 40; // 8 slot + 32 hash
  const target = BigInt(seedSlot);
  let fallback: Buffer | null = null;

  for (let i = 0; i < Math.min(count, 512); i++) {
    const off = 8 + i * ENTRY;
    if (off + ENTRY > data.length) break;
    const slot = data.readBigUInt64LE(off);
    const hash = Buffer.from(data.subarray(off + 8, off + ENTRY));
    if (!fallback) fallback = hash;
    if (slot === target) return hash;
  }
  if (!fallback) throw new Error(`no slot hash found for slot ${seedSlot}`);
  return fallback; // not yet finalized -> caller should retry until exact match
}

/** Wait (poll) until the chain slot is past `seedSlot`. */
export async function waitForSlot(conn: Connection, seedSlot: number, pollMs = 400): Promise<void> {
  // eslint-disable-next-line no-constant-condition
  while (true) {
    if ((await getCurrentSlot(conn)) > seedSlot) return;
    await new Promise((r) => setTimeout(r, pollMs));
  }
}

/** Exact slot hash for `seedSlot`, or null if it is not (yet) in the window. */
export async function fetchSeedExact(conn: Connection, seedSlot: number | bigint): Promise<Buffer | null> {
  const acc = await conn.getAccountInfo(SYSVAR_SLOT_HASHES, "confirmed");
  if (!acc) return null;
  const data = acc.data;
  const count = Number(data.readBigUInt64LE(0));
  const ENTRY = 40;
  const target = BigInt(seedSlot);
  for (let i = 0; i < Math.min(count, 512); i++) {
    const off = 8 + i * ENTRY;
    if (off + ENTRY > data.length) break;
    if (data.readBigUInt64LE(off) === target) {
      return Buffer.from(data.subarray(off + 8, off + ENTRY));
    }
  }
  return null;
}

/**
 * Wait for `seedSlot` to pass, then poll until its EXACT slot hash is available.
 * This is the verifiable lottery seed (deterministic, recomputable by anyone
 * while the slot is still in the SlotHashes window).
 */
export async function awaitSeed(
  conn: Connection,
  seedSlot: number,
  pollMs = 300,
  maxTries = 300
): Promise<Buffer> {
  await waitForSlot(conn, seedSlot);
  for (let i = 0; i < maxTries; i++) {
    const s = await fetchSeedExact(conn, seedSlot);
    if (s) return s;
    await new Promise((r) => setTimeout(r, pollMs));
  }
  throw new Error(`slot hash for slot ${seedSlot} not found in the SlotHashes window`);
}

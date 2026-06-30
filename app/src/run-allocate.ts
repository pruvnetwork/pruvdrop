/**
 * CLI — Layer 3 allocation over a committed snapshot.
 *
 * Usage:
 *   npx tsx src/run-allocate.ts [snapshot.json] [allocation.json]
 *
 * allocation.json (optional; defaults shown):
 *   { "mode": "top-n" | "weighted-lottery",
 *     "winners": 50, "totalAmount": "1000000000",
 *     "payout": "equal" | "proportional" }
 *
 * weighted-lottery needs a 32-byte seed = the on-chain slot hash AFTER the
 * commit slot. Provide it via SEED_HEX (64 hex chars). In production, fetch it
 * with `fetchSlotHashForSlot(connection, commitSlot)` from ../sdk once the slot
 * has passed — that is the value the on-chain program will use.
 *
 * Writes out/allocation.json (the input to Layer 4 / Merkle-drop) and self-verifies.
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import type { Snapshot } from "./types.js";
import { allocate, verifyAllocation, type AllocationConfig } from "./allocate.js";

function loadAllocCfg(path: string): AllocationConfig {
  const def = { mode: "top-n", winners: 50, totalAmount: "1000000000", payout: "equal" };
  const raw = existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : {};
  const m = { ...def, ...raw };
  return {
    mode: m.mode,
    winners: Number(m.winners),
    totalAmount: BigInt(m.totalAmount),
    payout: m.payout,
  };
}

function main() {
  const snapPath = process.argv[2] ?? "out/snapshot.json";
  const cfgPath = process.argv[3] ?? "allocation.json";
  const snapshot: Snapshot = JSON.parse(readFileSync(snapPath, "utf8"));
  const cfg = loadAllocCfg(cfgPath);

  let seed: Buffer | undefined;
  if (cfg.mode === "weighted-lottery") {
    const hex = process.env.SEED_HEX;
    if (!hex || hex.length !== 64) {
      throw new Error("weighted-lottery needs SEED_HEX=<64 hex chars> (on-chain slot hash after commit slot)");
    }
    seed = Buffer.from(hex, "hex");
  }

  console.log(`\nAllocation — mode=${cfg.mode} payout=${cfg.payout} winners=${cfg.winners}`);
  console.log(`Snapshot root: ${snapshot.merkleRoot}  (${snapshot.leafCount} candidates)`);
  if (seed) console.log(`Seed: ${seed.toString("hex")}`);

  const result = allocate(snapshot, cfg, seed);
  const ok = verifyAllocation(snapshot, cfg, result, seed);

  mkdirSync("out", { recursive: true });
  writeFileSync("out/allocation.json", JSON.stringify(result, null, 2));

  console.log(`\nWinners: ${result.winners}   distributed: ${result.totalDistributed}/${result.totalAmount}`);
  console.log(`Self-verify: ${ok ? "OK (anyone can recompute this from the committed snapshot + seed)" : "FAILED"}`);
  console.log("\nTop awards:");
  for (const a of result.awards.slice(0, 10)) {
    console.log(`  @${a.username} (fid ${a.fid})  ${a.amount}  ${a.rank ? `rank ${a.rank}` : `draw ${a.drawOrder}`}  score=${a.score.toFixed(3)}`);
  }
  console.log("\nWrote out/allocation.json  (-> Layer 4: build claim Merkle tree + Solana claim program)\n");
  if (!ok) process.exit(1);
}

main();

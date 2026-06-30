/**
 * Live end-to-end test for the verifiable lottery seed wiring.
 *
 * Requires a running validator with viral-airdrop-claim deployed.
 *   RPC=http://127.0.0.1:8899 ANCHOR_WALLET=~/.config/solana/id.json \
 *     npx tsx src/lottery-e2e.live-test.ts
 *
 * Proves: snapshot root committed BEFORE the seed slot, allocation seeded by the
 * real on-chain slot hash, on-chain commitment matches, and anyone can recompute.
 */
import anchorDefault from "@coral-xyz/anchor";
const anchor: any = (anchorDefault as any).default ?? anchorDefault;
const BN: any = anchor.BN;
import { Connection, Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { DEFAULT_CONFIG, type Candidate, type CampaignConfig } from "./types.js";
import { buildSnapshot } from "./snapshot.js";
import { allocateWeightedLottery, type AllocationConfig } from "./allocate.js";
import { runVerifiableLottery, verifyVerifiableLottery } from "./lottery-e2e.js";

function assert(c: boolean, m: string) { if (!c) { console.error("FAIL:", m); process.exit(1); } }

const RPC = process.env.RPC ?? "http://127.0.0.1:8899";
const WALLET = process.env.ANCHOR_WALLET ?? `${homedir()}/.config/solana/id.json`;
const IDL_PATH = new URL("../../target/idl/viral_airdrop_claim.json", import.meta.url);

async function main() {
  const conn = new Connection(RPC, "confirmed");
  const kp = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(WALLET, "utf8"))));
  const provider = new anchor.AnchorProvider(conn, new anchor.Wallet(kp), { commitment: "confirmed" });
  const idl = JSON.parse(readFileSync(IDL_PATH, "utf8"));
  const program = new anchor.Program(idl, provider);

  // Snapshot (mock candidates with varied scores).
  const camp: CampaignConfig = { ...DEFAULT_CONFIG, query: "$PRUV", windowStart: 0, windowEnd: 1 };
  const cands: Candidate[] = Array.from({ length: 12 }, (_, i) => ({
    fid: 200 + i, username: `c${i}`, wallet: `W${200 + i}`, score: (12 - i) * 5 + (i % 4),
    castCount: 1, totalLikes: 0, totalRecasts: 0, totalReplies: 0, qualityScore: 0.8,
  }));
  const snapshot = buildSnapshot(cands, camp);
  const cfg: AllocationConfig = { mode: "weighted-lottery", winners: 5, totalAmount: 1_000_000n, payout: "equal" };

  const [commitPda] = PublicKey.findProgramAddressSync(
    [Buffer.from("commitment"), Buffer.from(snapshot.merkleRoot, "hex")], program.programId);

  const commit = async (rootBytes: number[], seedSlot: number) => {
    await program.methods.commitSnapshot(rootBytes, new BN(seedSlot))
      .accounts({ authority: kp.publicKey, commitment: commitPda, systemProgram: SystemProgram.programId })
      .rpc();
  };

  console.log("Running verifiable lottery (commit-before-seed -> on-chain slot hash -> allocate)...");
  const { seedSlot, seed, result } = await runVerifiableLottery({ connection: conn, snapshot, cfg, commit, delaySlots: 12 });
  console.log(`  seedSlot=${seedSlot}  seed=${seed.toString("hex").slice(0, 16)}...  winners=${result.winners}`);

  // On-chain commitment matches what we allocated from.
  const c = await (program.account as any).snapshotCommitment.fetch(commitPda);
  assert(Number(c.seedSlot) === seedSlot, "on-chain seedSlot matches");
  assert(Buffer.from(c.snapshotRoot).toString("hex") === snapshot.merkleRoot, "on-chain snapshot root matches");

  // Independent verification (anyone re-fetches seed + recomputes).
  const ok = await verifyVerifiableLottery({ connection: conn, snapshot, cfg, seedSlot, result });
  assert(ok, "independent verification passes");

  // Determinism: same on-chain seed -> identical result.
  const r2 = allocateWeightedLottery(snapshot, cfg, seed);
  assert(JSON.stringify(r2.awards) === JSON.stringify(result.awards), "deterministic given the on-chain seed");

  const fids = result.awards.map((a) => a.fid);
  assert(new Set(fids).size === fids.length, "winners distinct");

  console.log("\n  Winners (weighted by score, seeded by on-chain slot hash):");
  for (const a of result.awards) console.log(`    @${a.username} (fid ${a.fid})  amount=${a.amount}  draw ${a.drawOrder}  score=${a.score}`);

  console.log("\nPASS — lottery seed wired end-to-end:");
  console.log("  committed before the seed existed, seeded by the on-chain slot hash, independently verifiable.");
}

main().catch((e) => { console.error("\nFailed:", e?.message ?? e); process.exit(1); });

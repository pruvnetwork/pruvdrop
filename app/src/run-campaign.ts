/**
 * CLI orchestrator — Layer 1 + Layer 2.
 *
 *   ingest (Farcaster/Neynar)  ->  score + filter  ->  canonical snapshot + Merkle root
 *
 * Usage:
 *   NEYNAR_API_KEY=... npx tsx src/run-campaign.ts ./campaign.json
 *
 * Writes:
 *   out/candidates.json  — the aggregated candidate set
 *   out/snapshot.json    — canonical snapshot + Merkle root (commit the root on-chain)
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { DEFAULT_CONFIG, type CampaignConfig } from "./types.js";
import { ingestCasts } from "./ingest.js";
import { scoreAndAggregate } from "./score.js";
import { buildSnapshot } from "./snapshot.js";

function loadConfig(path: string): CampaignConfig {
  const raw = JSON.parse(readFileSync(path, "utf8"));
  if (!raw.query) throw new Error("campaign config must include 'query'");
  if (!raw.windowStart || !raw.windowEnd) throw new Error("config needs windowStart/windowEnd (unix seconds)");
  return { ...DEFAULT_CONFIG, ...raw } as CampaignConfig;
}

async function main() {
  const path = process.argv[2] ?? "./campaign.json";
  const cfg = loadConfig(path);

  console.log(`\nViral Airdrop — campaign "${cfg.query}"`);
  console.log(`Window: ${new Date(cfg.windowStart * 1000).toISOString()} -> ${new Date(cfg.windowEnd * 1000).toISOString()}`);

  console.log("\n[1/3] Ingesting Farcaster casts (Neynar)...");
  const casts = await ingestCasts(cfg);
  console.log(`      pulled ${casts.length} casts in window`);

  console.log("[2/3] Scoring + filtering...");
  const { candidates, pending, stats } = scoreAndAggregate(casts, cfg);
  console.log(`      gates: ${stats.afterGates}/${stats.rawCasts} casts passed`);
  console.log(`      dropped — quality: ${stats.droppedQuality}, followers: ${stats.droppedFollowers}`);
  console.log(`      eligible authors (Solana wallet): ${stats.uniqueAuthors}`);
  console.log(`      pending authors (no Solana wallet, verify to qualify): ${stats.pendingAuthors}`);

  console.log("[3/3] Building canonical snapshot + Merkle root...");
  const snapshot = buildSnapshot(candidates, cfg);

  mkdirSync("out", { recursive: true });
  writeFileSync("out/candidates.json", JSON.stringify(candidates, null, 2));
  writeFileSync("out/snapshot.json", JSON.stringify(snapshot, null, 2));
  writeFileSync("out/pending.json", JSON.stringify(pending, null, 2));

  const top = [...candidates].sort((a, b) => b.score - a.score).slice(0, 10);
  console.log(`\nMerkle root: ${snapshot.merkleRoot}`);
  console.log(`Leaves: ${snapshot.leafCount}`);
  console.log("\nTop 10 by virality score:");
  for (const [i, c] of top.entries()) {
    console.log(`  ${String(i + 1).padStart(2)}. @${c.username} (fid ${c.fid})  score=${c.score.toFixed(3)}  L/R/Re=${c.totalLikes}/${c.totalRecasts}/${c.totalReplies}`);
  }
  if (pending.length > 0) {
    const tp = pending.slice(0, 5);
    console.log(`\nVerify-to-qualify — ${pending.length} casters are otherwise eligible but have no Solana wallet.`);
    console.log("Notify them to verify a Solana address on Farcaster before the snapshot is finalized:");
    for (const p of tp) console.log(`  @${p.username} (fid ${p.fid})  score=${p.score.toFixed(3)}`);
    console.log("  -> out/pending.json (full list)");
  }

  console.log(`\nWrote out/candidates.json, out/snapshot.json, out/pending.json`);
  console.log("Next: open a grace window (notify pending casters), re-run to pick up newly-verified");
  console.log("wallets, then commit snapshot.merkleRoot on-chain (PRUV) and run allocation.\n");
}

main().catch((e) => {
  console.error("\nFailed:", e?.message ?? e);
  process.exit(1);
});

/**
 * CLI — claim-based X campaign (wallet-in-tweet).
 *
 *   npx tsx src/run-campaign-x.ts ./tweets.json ./campaign-x.json
 *
 * tweets.json: JSON array of tweet URLs/ids, OR a newline-separated text file.
 * campaign-x.json: same shape as campaign.json (query = ticker; set
 *   minQualityScore: 0 since X has no quality score).
 *
 * Reuses the Farcaster path's scoring + snapshot + verify-to-qualify:
 *   eligible  = tweet has ticker + engagement + a valid Solana address
 *   pending   = tweet has ticker + engagement but NO Solana address
 *               ("edit/repost including your Solana address to qualify")
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { DEFAULT_CONFIG, type CampaignConfig } from "./types.js";
import { ingestXTweets } from "./ingest-x.js";
import { scoreAndAggregate } from "./score.js";
import { buildSnapshot } from "./snapshot.js";

function loadTweetRefs(path: string): string[] {
  const raw = readFileSync(path, "utf8").trim();
  if (raw.startsWith("[")) return JSON.parse(raw);
  return raw.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
}
function loadConfig(path: string): CampaignConfig {
  const raw = JSON.parse(readFileSync(path, "utf8"));
  if (!raw.query) throw new Error("campaign config must include 'query' (the ticker)");
  return { ...DEFAULT_CONFIG, minQualityScore: 0, windowStart: 1, windowEnd: 9999999999, ...raw } as CampaignConfig;
}

async function main() {
  const tweetsPath = process.argv[2] ?? "./tweets.json";
  const cfgPath = process.argv[3] ?? "./campaign-x.json";
  const refs = loadTweetRefs(tweetsPath);
  const cfg = loadConfig(cfgPath);

  console.log(`\nViral Airdrop (X) — ticker "${cfg.query}"`);
  console.log(`Submitted tweets: ${refs.length}`);

  console.log("\n[1/3] Reading tweets via syndication (no paid API)...");
  const casts = await ingestXTweets(refs, { ticker: cfg.query, windowStart: cfg.windowStart, windowEnd: cfg.windowEnd });
  console.log(`      ${casts.length} tweets matched the ticker and were readable`);

  console.log("[2/3] Scoring + filtering...");
  const { candidates, pending, stats } = scoreAndAggregate(casts, cfg);
  console.log(`      eligible authors (wallet in tweet): ${stats.uniqueAuthors}`);
  console.log(`      pending authors (no Solana address in tweet): ${stats.pendingAuthors}`);

  console.log("[3/3] Building canonical snapshot + Merkle root...");
  const snapshot = buildSnapshot(candidates, cfg);

  mkdirSync("out", { recursive: true });
  writeFileSync("out/candidates.json", JSON.stringify(candidates, null, 2));
  writeFileSync("out/snapshot.json", JSON.stringify(snapshot, null, 2));
  writeFileSync("out/pending.json", JSON.stringify(pending, null, 2));

  const top = [...candidates].sort((a, b) => b.score - a.score).slice(0, 10);
  console.log(`\nMerkle root: ${snapshot.merkleRoot}  (${snapshot.leafCount} leaves)`);
  console.log("Top by virality:");
  for (const [i, c] of top.entries()) {
    console.log(`  ${String(i + 1).padStart(2)}. @${c.username}  score=${c.score.toFixed(3)}  L/R/Re=${c.totalLikes}/${c.totalRecasts}/${c.totalReplies}  -> ${c.wallet.slice(0, 6)}…`);
  }
  if (pending.length > 0) {
    console.log(`\nVerify-to-qualify — ${pending.length} authors have the ticker + engagement but no Solana address in their tweet.`);
    console.log("Tell them: repost including your Solana address to qualify. (out/pending.json)");
  }
  console.log("\nNext: commit snapshot.merkleRoot on-chain, then run allocation + claim tree.\n");
}

main().catch((e) => { console.error("\nFailed:", e?.message ?? e); process.exit(1); });

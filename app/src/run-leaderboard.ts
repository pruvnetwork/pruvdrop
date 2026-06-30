/**
 * Generate leaderboard.json from a scored candidate set.
 *
 *   npx tsx src/run-leaderboard.ts [out/candidates.json] [out/leaderboard.json]
 *
 * Works for both Farcaster and X candidates (username = handle). Run it after
 * run-campaign / run-campaign-x to refresh live standings during a campaign.
 */
import { readFileSync, writeFileSync } from "node:fs";
import type { Candidate } from "./types.js";

const inPath = process.argv[2] ?? "out/candidates.json";
const outPath = process.argv[3] ?? "out/leaderboard.json";

const cands: Candidate[] = JSON.parse(readFileSync(inPath, "utf8"));
const rows = [...cands]
  .sort((a, b) => b.score - a.score)
  .map((c) => ({
    handle: c.username,
    posts: c.castCount,
    likes: c.totalLikes,
    score: Number(c.score.toFixed(1)),
  }));

writeFileSync(outPath, JSON.stringify(rows, null, 2));
console.log(`Leaderboard: ${rows.length} entries -> ${outPath}`);
if (rows[0]) console.log(`  #1 @${rows[0].handle}  score=${rows[0].score}`);

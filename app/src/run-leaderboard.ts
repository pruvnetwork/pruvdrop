/**
 * Generate leaderboard.json from a scored campaign run.
 *
 *   npx tsx src/run-leaderboard.ts [out/candidates.json] [out/leaderboard.json]
 *
 * Merges eligible candidates + verify-to-qualify pending authors (the leaderboard
 * shows virality standings regardless of wallet status). Works for Farcaster and X.
 * Run after run-campaign / run-campaign-x to refresh live standings.
 */
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import type { Candidate } from "./types.js";
import type { PendingCaster } from "./score.js";

const inPath = process.argv[2] ?? "out/candidates.json";
const outPath = process.argv[3] ?? "out/leaderboard.json";

const cands: Candidate[] = JSON.parse(readFileSync(inPath, "utf8"));
let all: Array<Candidate | PendingCaster> = [...cands];

// merge pending (otherwise-eligible authors without a wallet yet)
const pendPath = inPath.replace("candidates", "pending");
if (existsSync(pendPath)) {
  all = all.concat(JSON.parse(readFileSync(pendPath, "utf8")) as PendingCaster[]);
}

const rows = all
  .sort((a, b) => b.score - a.score)
  .map((c) => ({
    handle: c.username,
    posts: c.castCount,
    likes: c.totalLikes,
    score: Number(c.score.toFixed(1)),
  }));

writeFileSync(outPath, JSON.stringify(rows, null, 2));
console.log(`Leaderboard: ${rows.length} entries (${cands.length} eligible + ${rows.length - cands.length} pending) -> ${outPath}`);
if (rows[0]) console.log(`  #1 @${rows[0].handle}  score=${rows[0].score}  likes=${rows[0].likes}`);

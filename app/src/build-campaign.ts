/**
 * Assemble the web app's public campaign files from a completed run.
 *
 *   MINT=<mint> RPC=<rpc> [PROGRAM_ID=...] npx tsx src/build-campaign.ts
 *
 * Reads:
 *   out/claims.json       (claim tree: merkleRoot + claims[])  <- run-claimtree
 *   out/candidates.json   (scored authors)                      <- run-campaign(-x)
 * Writes (into ../web/public):
 *   config.json     { programId, rpcUrl, mint, claimRoot }
 *   claims.json     { walletBase58: { index, amount, proof } }
 *   leaderboard.json [ { handle, posts, likes, score } ]
 *
 * Then commit web/public and redeploy.
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import type { Candidate } from "./types.js";
import type { ClaimTree } from "./claim-tree.js";

const PROGRAM_ID = process.env.PROGRAM_ID ?? "3oCMjxiXMorGrFUrFqYUmpfwG1FMLaLBWJBh6pVRcLqJ";
const RPC = process.env.RPC ?? "https://api.devnet.solana.com";
const MINT = process.env.MINT;
const WEB = process.env.WEB_PUBLIC ?? "../web/public";

function main() {
  if (!MINT) throw new Error("set MINT=<token mint>");
  if (!existsSync("out/claims.json")) throw new Error("out/claims.json not found — run run-claimtree first");
  mkdirSync(WEB, { recursive: true });

  const tree: ClaimTree = JSON.parse(readFileSync("out/claims.json", "utf8"));
  const lookup: Record<string, { index: number; amount: string; proof: string[] }> = {};
  for (const c of tree.claims) lookup[c.wallet] = { index: c.index, amount: c.amount, proof: c.proof };

  writeFileSync(`${WEB}/config.json`, JSON.stringify(
    { programId: PROGRAM_ID, rpcUrl: RPC, mint: MINT, claimRoot: tree.merkleRoot }, null, 2));
  writeFileSync(`${WEB}/claims.json`, JSON.stringify(lookup, null, 2));

  let lbCount = 0;
  if (existsSync("out/candidates.json")) {
    const cands: Candidate[] = JSON.parse(readFileSync("out/candidates.json", "utf8"));
    const rows = [...cands].sort((a, b) => b.score - a.score).map((c) => ({
      handle: c.username, posts: c.castCount, likes: c.totalLikes, score: Number(c.score.toFixed(1)),
    }));
    writeFileSync(`${WEB}/leaderboard.json`, JSON.stringify(rows, null, 2));
    lbCount = rows.length;
  }

  console.log(`Wrote ${WEB}/config.json, claims.json (${tree.count} recipients), leaderboard.json (${lbCount}).`);
  console.log(`  mint: ${MINT}`);
  console.log(`  claimRoot: ${tree.merkleRoot}`);
  console.log("Commit web/public and redeploy.");
}

main();

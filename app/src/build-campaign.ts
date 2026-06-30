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
 * And (into out/):
 *   allocation-input.json { t, pot, rows[{wallet,score,amount}] }  <- for `prover --allocation`
 *
 * Then commit web/public and redeploy; run the ZK prover on allocation-input.json.
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

    // leaderboard
    const rows = [...cands].sort((a, b) => b.score - a.score).map((c) => ({
      handle: c.username, posts: c.castCount, likes: c.totalLikes, score: Number(c.score.toFixed(1)),
    }));
    writeFileSync(`${WEB}/leaderboard.json`, JSON.stringify(rows, null, 2));
    lbCount = rows.length;

    // allocation-input.json for the ZK allocation prover (top-N mode).
    // score scaled to an integer (<65536, the circuit's comparator width); amount = claim if won, else 0.
    const SCALE = Number(process.env.SCORE_SCALE ?? "10");
    // cap candidate count for a single proof (large M needs the recursion path, not yet built)
    const MAX = Number(process.env.MAX_CANDIDATES ?? "0");
    const winAmt = new Map<string, number>();
    for (const c of tree.claims) winAmt.set(c.wallet, Number(c.amount));
    let arows = [...cands]
      .filter((c) => c.wallet)
      .sort((a, b) => b.score - a.score)
      .map((c) => ({
        wallet: c.wallet,
        score: Math.min(65535, Math.max(0, Math.round(c.score * SCALE))),
        amount: winAmt.get(c.wallet) ?? 0,
      }));
    if (MAX > 0 && arows.length > MAX) arows = arows.slice(0, MAX); // top-M by score (winners are top scores)
    const winS = arows.filter((r) => r.amount > 0).map((r) => r.score);
    const loseS = arows.filter((r) => r.amount === 0).map((r) => r.score);
    const maxLose = loseS.length ? Math.max(...loseS) : -1;
    const minWin = winS.length ? Math.min(...winS) : 0;
    const t = maxLose + 1; // winners = { score >= t } = { score > maxLoser }
    const pot = arows.reduce((s, r) => s + r.amount, 0);
    if (winS.length && minWin < t) {
      console.warn("  ⚠ allocation-input: winner/loser scores overlap (ties or non-top-N) — ZK top-N proof needs tie-break or the lottery circuit");
    }
    writeFileSync("out/allocation-input.json", JSON.stringify({ t, pot, rows: arows }, null, 2));
    console.log(`Wrote out/allocation-input.json (M=${arows.length}, N=${winS.length}, t=${t}, pot=${pot}).`);
  }

  console.log(`Wrote ${WEB}/config.json, claims.json (${tree.count} recipients), leaderboard.json (${lbCount}).`);
  console.log(`  mint: ${MINT}`);
  console.log(`  claimRoot: ${tree.merkleRoot}`);
  console.log("Commit web/public + redeploy; then: cd ../prover && cargo run --release -- --allocation");
}

main();

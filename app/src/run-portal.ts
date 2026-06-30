/**
 * Generate the static claim portal's data files from the claim tree.
 *
 *   MINT=<mint> RPC=<url> npx tsx src/run-portal.ts [out/claims.json]
 *
 * Writes:
 *   portal/config.json  — { programId, rpcUrl, mint, claimRoot }
 *   portal/claims.json  — { [walletBase58]: { index, amount, proof[] } }
 *
 * Then serve the portal/ directory (any static host) and recipients claim
 * by connecting the wallet that holds their award.
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import type { ClaimTree } from "./claim-tree.js";

const PROGRAM_ID = process.env.PROGRAM_ID ?? "3oCMjxiXMorGrFUrFqYUmpfwG1FMLaLBWJBh6pVRcLqJ";
const RPC = process.env.RPC ?? "https://api.devnet.solana.com";
const MINT = process.env.MINT;

function main() {
  if (!MINT) throw new Error("set MINT=<token mint address>");
  const path = process.argv[2] ?? "out/claims.json";
  const tree: ClaimTree = JSON.parse(readFileSync(path, "utf8"));

  const lookup: Record<string, { index: number; amount: string; proof: string[] }> = {};
  for (const c of tree.claims) {
    lookup[c.wallet] = { index: c.index, amount: c.amount, proof: c.proof };
  }

  mkdirSync("portal", { recursive: true });
  writeFileSync(
    "portal/config.json",
    JSON.stringify({ programId: PROGRAM_ID, rpcUrl: RPC, mint: MINT, claimRoot: tree.merkleRoot }, null, 2)
  );
  writeFileSync("portal/claims.json", JSON.stringify(lookup, null, 2));

  console.log(`portal/config.json + portal/claims.json written (${tree.count} recipients)`);
  console.log(`  programId: ${PROGRAM_ID}`);
  console.log(`  mint:      ${MINT}`);
  console.log(`  claimRoot: ${tree.merkleRoot}`);
  console.log(`Serve the portal/ folder (e.g. \`npx serve portal\`) and recipients can claim.`);
}

main();

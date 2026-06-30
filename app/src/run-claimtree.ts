/**
 * CLI — build the claim Merkle tree from an allocation result.
 *
 *   npx tsx src/run-claimtree.ts [allocation.json]
 *
 * Writes out/claims.json: { merkleRoot, claims:[{index,wallet,amount,proof[]}] }.
 * Use merkleRoot to `initialize` the on-chain distributor; each recipient calls
 * `claim(index, amount, proof)`.
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import type { AllocationResult } from "./allocate.js";
import { buildClaimTree } from "./claim-tree.js";

function main() {
  const path = process.argv[2] ?? "out/allocation.json";
  const alloc: AllocationResult = JSON.parse(readFileSync(path, "utf8"));
  const tree = buildClaimTree(alloc.awards);

  mkdirSync("out", { recursive: true });
  writeFileSync("out/claims.json", JSON.stringify(tree, null, 2));

  console.log(`\nClaim tree built from ${tree.count} awards`);
  console.log(`Claim Merkle root: ${tree.merkleRoot}`);
  console.log("  -> initialize the on-chain distributor with this root");
  console.log("  -> each recipient calls claim(index, amount, proof)");
  console.log("\nWrote out/claims.json\n");
}

main();

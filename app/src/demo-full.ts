/**
 * Full-chain demo over the REAL Farcaster snapshot.
 *
 *   real snapshot -> allocate (top-N) -> claim tree -> token mint ->
 *   initialize distributor -> fund vault -> live claim -> portal files
 *
 * A demo recipient (the payer wallet) is appended so we can execute a real
 * on-chain claim and show the transaction. The real Farcaster recipients remain
 * claimable via the portal with their own wallets.
 *
 *   RPC=http://127.0.0.1:8899 ANCHOR_WALLET=~/.config/solana/id.json \
 *     npx tsx src/demo-full.ts
 */
import anchorDefault from "@coral-xyz/anchor";
const anchor: any = (anchorDefault as any).default ?? anchorDefault;
import {
  Connection, Keypair, PublicKey, Transaction, TransactionInstruction,
  SystemProgram, SYSVAR_RENT_PUBKEY,
} from "@solana/web3.js";
import { createMint, mintTo, getAccount, TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import type { Snapshot } from "./types.js";
import type { Award } from "./allocate.js";
import { allocateTopN, type AllocationConfig } from "./allocate.js";
import { buildClaimTree } from "./claim-tree.js";

// portal-identical instruction helpers
const CLAIM_DISC = new Uint8Array([62, 198, 214, 193, 213, 159, 108, 210]);
const enc = new TextEncoder();
const hexToBytes = (h: string) => { const a = new Uint8Array(h.length / 2); for (let i = 0; i < a.length; i++) a[i] = parseInt(h.substr(i * 2, 2), 16); return a; };
const u64LE = (v: bigint | number) => { const b = new Uint8Array(8); new DataView(b.buffer).setBigUint64(0, BigInt(v), true); return b; };
const u32LE = (v: number) => { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, v >>> 0, true); return b; };
const cat = (...a: Uint8Array[]) => { let n = 0; for (const x of a) n += x.length; const o = new Uint8Array(n); let p = 0; for (const x of a) { o.set(x, p); p += x.length; } return o; };
const pda = (s: (Uint8Array | Buffer)[], pid: PublicKey) => PublicKey.findProgramAddressSync(s as any, pid)[0];
const ataFor = (o: PublicKey, m: PublicKey) => pda([o.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), m.toBuffer()], ASSOCIATED_TOKEN_PROGRAM_ID);
const createAtaIx = (payer: PublicKey, ata: PublicKey, owner: PublicKey, mint: PublicKey) => new TransactionInstruction({
  programId: ASSOCIATED_TOKEN_PROGRAM_ID,
  keys: [
    { pubkey: payer, isSigner: true, isWritable: true }, { pubkey: ata, isSigner: false, isWritable: true },
    { pubkey: owner, isSigner: false, isWritable: false }, { pubkey: mint, isSigner: false, isWritable: false },
    { pubkey: SystemProgram.programId, isSigner: false, isWritable: false }, { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
  ], data: Buffer.from([1]),
});
function claimIx(programId: PublicKey, mint: PublicKey, root: string, c: { index: number; amount: string; proof: string[] }, claimant: PublicKey) {
  const distributor = pda([enc.encode("distributor"), hexToBytes(root)], programId);
  const vault = pda([enc.encode("vault"), distributor.toBuffer()], programId);
  const claimStatus = pda([enc.encode("claim"), distributor.toBuffer(), u64LE(c.index)], programId);
  const pb = c.proof.map(hexToBytes);
  const data = cat(CLAIM_DISC, u64LE(c.index), u64LE(BigInt(c.amount)), u32LE(pb.length), ...pb);
  return new TransactionInstruction({
    programId, data: Buffer.from(data),
    keys: [
      { pubkey: claimant, isSigner: true, isWritable: true },
      { pubkey: distributor, isSigner: false, isWritable: true },
      { pubkey: vault, isSigner: false, isWritable: true },
      { pubkey: ataFor(claimant, mint), isSigner: false, isWritable: true },
      { pubkey: claimStatus, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
  });
}

const RPC = process.env.RPC ?? "http://127.0.0.1:8899";
const WALLET = process.env.ANCHOR_WALLET ?? `${homedir()}/.config/solana/id.json`;
const IDL_PATH = new URL("../../target/idl/viral_airdrop_claim.json", import.meta.url);

async function main() {
  const conn = new Connection(RPC, "confirmed");
  const payer = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(WALLET, "utf8"))));
  const provider = new anchor.AnchorProvider(conn, new anchor.Wallet(payer), { commitment: "confirmed" });
  const idl = JSON.parse(readFileSync(IDL_PATH, "utf8"));
  const program = new anchor.Program(idl, provider);
  const programId = program.programId as PublicKey;

  // 1. real snapshot -> allocate top-N
  const snapshot: Snapshot = JSON.parse(readFileSync("out/snapshot.json", "utf8"));
  console.log(`Real snapshot: ${snapshot.leafCount} Farcaster authors, root ${snapshot.merkleRoot.slice(0, 12)}…`);
  const allocCfg: AllocationConfig = { mode: "top-n", winners: 20, totalAmount: 20_000_000n, payout: "proportional" };
  const alloc = allocateTopN(snapshot, allocCfg);
  console.log(`Allocated to top ${alloc.winners} authors (proportional, 20 tokens).`);

  // 2. append a demo recipient (payer) so we can show a live claim
  const demo: Award = { fid: 0, username: "demo-claimer", wallet: payer.publicKey.toBase58(), amount: "1000000", score: 0 };
  const awards = [...alloc.awards, demo];
  const tree = buildClaimTree(awards);
  const total = awards.reduce((a, x) => a + BigInt(x.amount), 0n);

  // 3. token mint
  const mint = await createMint(conn, payer, payer.publicKey, null, 6);
  console.log(`Token mint: ${mint.toBase58()}`);

  // 4. initialize distributor + vault, fund vault
  const rootArr = [...Buffer.from(tree.merkleRoot, "hex")];
  const distributor = pda([enc.encode("distributor"), hexToBytes(tree.merkleRoot)], programId);
  const vault = pda([enc.encode("vault"), distributor.toBuffer()], programId);
  await program.methods.initialize(rootArr).accounts({
    authority: payer.publicKey, mint, distributor, vault,
    tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId, rent: SYSVAR_RENT_PUBKEY,
  }).rpc();
  await mintTo(conn, payer, mint, vault, payer, total);
  console.log(`Distributor ${distributor.toBase58().slice(0, 8)}… funded with ${total} base units.`);

  // 5. live claim (the demo recipient = payer), via the portal's exact instruction
  const myClaim = tree.claims[tree.claims.length - 1];
  const ata = ataFor(payer.publicKey, mint);
  const tx = new Transaction();
  tx.add(createAtaIx(payer.publicKey, ata, payer.publicKey, mint));
  tx.add(claimIx(programId, mint, tree.merkleRoot, myClaim, payer.publicKey));
  tx.feePayer = payer.publicKey;
  tx.recentBlockhash = (await conn.getLatestBlockhash()).blockhash;
  tx.sign(payer);
  const sig = await conn.sendRawTransaction(tx.serialize());
  await conn.confirmTransaction(sig, "confirmed");
  const bal = (await getAccount(conn, ata)).amount.toString();
  if (bal !== demo.amount) throw new Error(`claim balance mismatch: ${bal} != ${demo.amount}`);

  // 6. portal files
  const lookup: Record<string, any> = {};
  for (const c of tree.claims) lookup[c.wallet] = { index: c.index, amount: c.amount, proof: c.proof };
  mkdirSync("portal", { recursive: true });
  writeFileSync("portal/config.json", JSON.stringify({ programId: programId.toBase58(), rpcUrl: RPC, mint: mint.toBase58(), claimRoot: tree.merkleRoot }, null, 2));
  writeFileSync("portal/claims.json", JSON.stringify(lookup, null, 2));

  const cluster = RPC.includes("devnet") ? "?cluster=devnet" : RPC.includes("127.0.0.1") ? "?cluster=custom" : "";
  console.log("\nPASS — full chain over real Farcaster data:");
  console.log(`  recipients in claim tree: ${tree.count} (20 real authors + 1 demo)`);
  console.log(`  claim Merkle root: ${tree.merkleRoot}`);
  console.log(`  live claim tx: https://explorer.solana.com/tx/${sig}${cluster}`);
  console.log(`  demo recipient received ${bal} base units (1.0 token)`);
  console.log(`  portal/config.json + portal/claims.json written (serve with: npx serve portal)`);
}

main().catch((e) => { console.error("\nFailed:", e?.message ?? e); process.exit(1); });

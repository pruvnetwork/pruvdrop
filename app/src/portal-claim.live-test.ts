/**
 * Live test that replicates the portal's MANUAL claim-instruction building
 * (index.html logic) and submits it to the deployed program, proving the
 * portal's transaction is correct end-to-end.
 *
 *   RPC=http://127.0.0.1:8899 ANCHOR_WALLET=~/.config/solana/id.json \
 *     npx tsx src/portal-claim.live-test.ts
 */
import anchorDefault from "@coral-xyz/anchor";
const anchor: any = (anchorDefault as any).default ?? anchorDefault;
import {
  Connection, Keypair, PublicKey, Transaction, TransactionInstruction,
  SystemProgram, SYSVAR_RENT_PUBKEY,
} from "@solana/web3.js";
import {
  createMint, mintTo, getAccount, TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import type { Award } from "./allocate.js";
import { buildClaimTree } from "./claim-tree.js";

function assert(c: boolean, m: string) { if (!c) { console.error("FAIL:", m); process.exit(1); } }

// ── portal helpers (identical to portal/index.html) ──────────────────────────
const CLAIM_DISC = new Uint8Array([62, 198, 214, 193, 213, 159, 108, 210]);
const enc = new TextEncoder();
const hexToBytes = (h: string) => { const a = new Uint8Array(h.length / 2); for (let i = 0; i < a.length; i++) a[i] = parseInt(h.substr(i * 2, 2), 16); return a; };
const u64LE = (v: bigint | number) => { const b = new Uint8Array(8); new DataView(b.buffer).setBigUint64(0, BigInt(v), true); return b; };
const u32LE = (v: number) => { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, v >>> 0, true); return b; };
const cat = (...arrs: Uint8Array[]) => { let n = 0; for (const a of arrs) n += a.length; const out = new Uint8Array(n); let o = 0; for (const a of arrs) { out.set(a, o); o += a.length; } return out; };
const pda = (seeds: (Uint8Array | Buffer)[], pid: PublicKey) => PublicKey.findProgramAddressSync(seeds as any, pid)[0];
const ataFor = (owner: PublicKey, mint: PublicKey) => pda([owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()], ASSOCIATED_TOKEN_PROGRAM_ID);

function createAtaIdempotentIx(payer: PublicKey, ata: PublicKey, owner: PublicKey, mint: PublicKey) {
  return new TransactionInstruction({
    programId: ASSOCIATED_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: ata, isSigner: false, isWritable: true },
      { pubkey: owner, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([1]),
  });
}

function claimIx(programId: PublicKey, mint: PublicKey, root: string, claim: { index: number; amount: string; proof: string[] }, claimant: PublicKey) {
  const distributor = pda([enc.encode("distributor"), hexToBytes(root)], programId);
  const vault = pda([enc.encode("vault"), distributor.toBuffer()], programId);
  const claimantToken = ataFor(claimant, mint);
  const claimStatus = pda([enc.encode("claim"), distributor.toBuffer(), u64LE(claim.index)], programId);
  const proofBytes = claim.proof.map(hexToBytes);
  const data = cat(CLAIM_DISC, u64LE(claim.index), u64LE(BigInt(claim.amount)), u32LE(proofBytes.length), ...proofBytes);
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: claimant, isSigner: true, isWritable: true },
      { pubkey: distributor, isSigner: false, isWritable: true },
      { pubkey: vault, isSigner: false, isWritable: true },
      { pubkey: claimantToken, isSigner: false, isWritable: true },
      { pubkey: claimStatus, isSigner: false, isWritable: true },
      { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.from(data),
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

  // mint + recipients + claim tree
  const mint = await createMint(conn, payer, payer.publicKey, null, 6);
  const recipients = [Keypair.generate(), Keypair.generate()];
  for (const r of recipients) await conn.confirmTransaction(await conn.requestAirdrop(r.publicKey, 1e9));
  const awards: Award[] = recipients.map((r, i) => ({
    fid: 300 + i, username: `r${i}`, wallet: r.publicKey.toBase58(),
    amount: String(1000 * (i + 1)), score: 10 - i,
  }));
  const tree = buildClaimTree(awards);
  const rootArr = [...Buffer.from(tree.merkleRoot, "hex")];

  // initialize distributor + vault (anchor), fund vault
  const distributor = pda([enc.encode("distributor"), hexToBytes(tree.merkleRoot)], programId);
  const vault = pda([enc.encode("vault"), distributor.toBuffer()], programId);
  await program.methods.initialize(rootArr).accounts({
    authority: payer.publicKey, mint, distributor, vault,
    tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId, rent: SYSVAR_RENT_PUBKEY,
  }).rpc();
  await mintTo(conn, payer, mint, vault, payer, 100000n);

  // claim via the PORTAL's manual instruction (signed by the recipient)
  async function portalClaim(i: number) {
    const r = recipients[i];
    const c = tree.claims[i];
    const ata = ataFor(r.publicKey, mint);
    const tx = new Transaction();
    tx.add(createAtaIdempotentIx(r.publicKey, ata, r.publicKey, mint));
    tx.add(claimIx(programId, mint, tree.merkleRoot, c, r.publicKey));
    tx.feePayer = r.publicKey;
    tx.recentBlockhash = (await conn.getLatestBlockhash()).blockhash;
    tx.sign(r);
    const sig = await conn.sendRawTransaction(tx.serialize());
    await conn.confirmTransaction(sig, "confirmed");
    return ata;
  }

  const ata0 = await portalClaim(0);
  assert((await getAccount(conn, ata0)).amount.toString() === awards[0].amount, "portal claim paid recipient 0");

  const ata1 = await portalClaim(1);
  assert((await getAccount(conn, ata1)).amount.toString() === awards[1].amount, "portal claim paid recipient 1");

  // double claim rejected
  let dbl = false;
  try { await portalClaim(0); } catch { dbl = true; }
  assert(dbl, "portal double-claim rejected");

  console.log("PASS — portal claim transaction is correct on-chain:");
  console.log(`  recipient 0 received ${awards[0].amount}, recipient 1 received ${awards[1].amount}, double-claim rejected.`);
}

main().catch((e) => { console.error("\nFailed:", e?.message ?? e); process.exit(1); });

/**
 * Integration test — viral-airdrop-claim.
 *
 * Covers: commit_snapshot (future-slot guard), initialize, fund vault,
 * claim (top-N, no seed needed), double-claim rejection, bad-proof rejection.
 *
 * Run with the program deployed to a local validator:
 *   anchor test            (uses Anchor.toml [scripts] test)
 * or against a running validator:
 *   anchor build -p viral_airdrop_claim && anchor deploy && \
 *   ts-mocha -p ./tsconfig.json -t 1000000 tests/viral-airdrop-claim.test.ts
 *
 * Requires: @solana/spl-token in the test deps.
 */
import anchorDefault from "@coral-xyz/anchor";
const anchor: any = (anchorDefault as any).default ?? anchorDefault;
const BN: any = anchor.BN;
import { PublicKey, Keypair, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import {
  createMint, getOrCreateAssociatedTokenAccount, mintTo, getAccount, TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { assert } from "chai";
import { createHash } from "node:crypto";

// ── claim-tree hashing (mirrors viral-airdrop/src/claim-tree.ts and the program) ──
const sha256 = (b: Buffer) => createHash("sha256").update(b).digest();
const u64le = (v: bigint) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(v); return b; };
function leafHash(index: number, wallet: PublicKey, amount: bigint): Buffer {
  return sha256(Buffer.concat([Buffer.from([0]), u64le(BigInt(index)), wallet.toBuffer(), u64le(amount)]));
}
function nodeHash(a: Buffer, b: Buffer): Buffer {
  const [lo, hi] = Buffer.compare(a, b) <= 0 ? [a, b] : [b, a];
  return sha256(Buffer.concat([Buffer.from([1]), lo, hi]));
}
function buildTree(leaves: Buffer[]) {
  const layers = [leaves];
  while (layers[layers.length - 1].length > 1) {
    const cur = layers[layers.length - 1]; const next: Buffer[] = [];
    for (let i = 0; i < cur.length; i += 2) next.push(nodeHash(cur[i], i + 1 < cur.length ? cur[i + 1] : cur[i]));
    layers.push(next);
  }
  return layers;
}
function proofFor(layers: Buffer[][], index: number): Buffer[] {
  const proof: Buffer[] = []; let idx = index;
  for (let l = 0; l < layers.length - 1; l++) {
    const layer = layers[l]; const sib = idx ^ 1;
    proof.push(sib < layer.length ? layer[sib] : layer[idx]); idx >>= 1;
  }
  return proof;
}

describe("viral-airdrop-claim", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.ViralAirdropClaim as anchor.Program;
  const conn = provider.connection;
  const authority = (provider.wallet as anchor.Wallet).payer;

  it("commit -> initialize -> claim x2, reject double-claim and bad proof", async () => {
    // 1. mint + recipients
    const mint = await createMint(conn, authority, authority.publicKey, null, 6);
    const recipients = [Keypair.generate(), Keypair.generate(), Keypair.generate()];
    for (const r of recipients) {
      await conn.confirmTransaction(await conn.requestAirdrop(r.publicKey, 1e9));
    }
    const amounts = [1000n, 2500n, 500n];

    // 2. claim tree
    const leaves = recipients.map((r, i) => leafHash(i, r.publicKey, amounts[i]));
    const layers = buildTree(leaves);
    const root = layers[layers.length - 1][0];
    const rootArr = [...root];

    // 3. commit-before-seed: future slot ok, past slot rejected
    const snapshotRoot = [...sha256(Buffer.from("snapshot"))];
    const [commitPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("commitment"), Buffer.from(snapshotRoot)], program.programId);
    const slot = await conn.getSlot();
    await program.methods.commitSnapshot(snapshotRoot, new BN(slot + 1000))
      .accounts({ authority: authority.publicKey, commitment: commitPda, systemProgram: SystemProgram.programId })
      .rpc();
    const commit = await (program.account as any).snapshotCommitment.fetch(commitPda);
    assert.equal(Number(commit.seedSlot), slot + 1000, "seed slot stored");

    let pastFailed = false;
    try {
      const sr2 = [...sha256(Buffer.from("snapshot2"))];
      const [c2] = PublicKey.findProgramAddressSync([Buffer.from("commitment"), Buffer.from(sr2)], program.programId);
      await program.methods.commitSnapshot(sr2, new BN(1))
        .accounts({ authority: authority.publicKey, commitment: c2, systemProgram: SystemProgram.programId }).rpc();
    } catch { pastFailed = true; }
    assert.isTrue(pastFailed, "past seed slot rejected (commit-before-seed)");

    // 4. initialize distributor + vault, fund vault
    const [distributor] = PublicKey.findProgramAddressSync(
      [Buffer.from("distributor"), Buffer.from(rootArr)], program.programId);
    const [vault] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), distributor.toBuffer()], program.programId);
    await program.methods.initialize(rootArr)
      .accounts({
        authority: authority.publicKey, mint, distributor, vault,
        tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId, rent: SYSVAR_RENT_PUBKEY,
      }).rpc();
    const authAta = await getOrCreateAssociatedTokenAccount(conn, authority, mint, authority.publicKey);
    await mintTo(conn, authority, mint, authAta.address, authority, 10000n);
    await mintTo(conn, authority, mint, vault, authority, 4000n); // fund vault directly

    // 5. claim for recipient 0 and 1
    async function claim(i: number, signer: Keypair) {
      const ata = await getOrCreateAssociatedTokenAccount(conn, signer, mint, signer.publicKey);
      const [claimStatus] = PublicKey.findProgramAddressSync(
        [Buffer.from("claim"), distributor.toBuffer(), u64le(BigInt(i))], program.programId);
      const proof = proofFor(layers, i).map((p) => [...p]);
      await program.methods.claim(new BN(i), new BN(amounts[i].toString()), proof)
        .accounts({
          claimant: signer.publicKey, distributor, vault, claimantToken: ata.address,
          claimStatus, tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
        }).signers([signer]).rpc();
      return ata.address;
    }

    const ata0 = await claim(0, recipients[0]);
    const ata1 = await claim(1, recipients[1]);
    assert.equal((await getAccount(conn, ata0)).amount.toString(), amounts[0].toString(), "recipient 0 paid");
    assert.equal((await getAccount(conn, ata1)).amount.toString(), amounts[1].toString(), "recipient 1 paid");

    // 6. double-claim rejected (nullifier PDA already exists)
    let doubleFailed = false;
    try { await claim(0, recipients[0]); } catch { doubleFailed = true; }
    assert.isTrue(doubleFailed, "double-claim rejected");

    // 7. bad proof rejected (recipient 2 claims with recipient 0's proof)
    let badFailed = false;
    try {
      const ata = await getOrCreateAssociatedTokenAccount(conn, recipients[2], mint, recipients[2].publicKey);
      const [cs] = PublicKey.findProgramAddressSync(
        [Buffer.from("claim"), distributor.toBuffer(), u64le(2n)], program.programId);
      const wrong = proofFor(layers, 0).map((p) => [...p]);
      await program.methods.claim(new BN(2), new BN(amounts[2].toString()), wrong)
        .accounts({
          claimant: recipients[2].publicKey, distributor, vault, claimantToken: ata.address,
          claimStatus: cs, tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
        }).signers([recipients[2]]).rpc();
    } catch { badFailed = true; }
    assert.isTrue(badFailed, "bad proof rejected");
  });
});

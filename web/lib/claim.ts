import { PublicKey, TransactionInstruction, SystemProgram } from "@solana/web3.js";

export const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
export const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const CLAIM_DISC = new Uint8Array([62, 198, 214, 193, 213, 159, 108, 210]);
const enc = new TextEncoder();

export interface AirdropConfig {
  programId: string;
  rpcUrl: string;
  mint: string;
  claimRoot: string;
}
export interface ClaimEntry {
  index: number;
  amount: string;
  proof: string[];
}
export type ClaimsMap = Record<string, ClaimEntry>;

function hexToBytes(h: string): Uint8Array {
  const a = new Uint8Array(h.length / 2);
  for (let i = 0; i < a.length; i++) a[i] = parseInt(h.substr(i * 2, 2), 16);
  return a;
}
function u64LE(v: bigint | number): Uint8Array {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, BigInt(v), true);
  return b;
}
function u32LE(v: number): Uint8Array {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, v >>> 0, true);
  return b;
}
function cat(...arrs: Uint8Array[]): Uint8Array {
  let n = 0;
  for (const a of arrs) n += a.length;
  const out = new Uint8Array(n);
  let o = 0;
  for (const a of arrs) { out.set(a, o); o += a.length; }
  return out;
}
function pda(seeds: (Uint8Array | Buffer)[], programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds as Buffer[], programId)[0];
}
export function ataFor(owner: PublicKey, mint: PublicKey): PublicKey {
  return pda([owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()], ASSOCIATED_TOKEN_PROGRAM_ID);
}

export function createAtaIdempotentIx(payer: PublicKey, ata: PublicKey, owner: PublicKey, mint: PublicKey): TransactionInstruction {
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

export function claimIx(cfg: AirdropConfig, claim: ClaimEntry, claimant: PublicKey): TransactionInstruction {
  const programId = new PublicKey(cfg.programId);
  const mint = new PublicKey(cfg.mint);
  const distributor = pda([enc.encode("distributor"), hexToBytes(cfg.claimRoot)], programId);
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

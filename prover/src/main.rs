//! pruvdrop — Phase-1 ZK inclusion prover.
//!
//! Builds a Poseidon (BN254) Merkle mirror of the winner allocation and produces a
//! **real** `pruv-circuits` Halo2 KZG inclusion proof for one winner, then
//! SHA-256-hashes the proof bytes (the artifact PRUV's attestation records on-chain).
//!
//! This is **Phase 1** of `docs/ZK-ALLOCATION-PROOF.md`: it proves
//! *"winner ∈ committed Poseidon tree"* — a parallel commitment to the SHA-256 claim
//! tree the on-chain claim program uses. It does **not** yet prove the allocation
//! *function* is correct; that is Phase 2 (the selection/payout circuit).
//!
//! Run (with a side-by-side `pruv-solana-main` checkout):
//!   cargo run --release -- --claims ../app/out/claims.json --index 0 --out ../web/public

use anyhow::{Context, Result};
use base64::Engine;
use clap::Parser;
use halo2_proofs::halo2curves::bn256::Fr;
use pruv_circuits::circuit_params::{fr_from_bytes, fr_to_bytes};
use pruv_circuits::merkle::{self, MerkleWitness, DEPTH};
use pruv_circuits::poseidon_hasher::{hash_two, leaf_commitment, merkle_root_from_path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

mod topn;

#[derive(Parser)]
struct Args {
    /// pruvdrop claim tree (out/claims.json from run-claimtree)
    #[arg(long, default_value = "../app/out/claims.json")]
    claims: PathBuf,
    /// winner index to prove inclusion for
    #[arg(long, default_value_t = 0)]
    index: usize,
    /// output directory for allocation-proof.{json,bin}
    #[arg(long, default_value = "../web/public")]
    out: PathBuf,
    /// run the Phase-2 top-N counting spike instead of the Phase-1 inclusion proof
    #[arg(long)]
    spike: bool,
}

/// pruvdrop claim-tree shape (subset we need).
#[derive(Deserialize)]
struct ClaimTree {
    claims: Vec<ClaimEntry>,
}
#[derive(Deserialize, Clone)]
struct ClaimEntry {
    index: u64,
    wallet: String,
    amount: String,
}

#[derive(Serialize)]
struct ProofOut {
    scheme: String,
    index: usize,
    wallet: String,
    leaf: String,
    poseidon_root: String,
    proof_hash: String,
    public_inputs: Vec<String>,
    proof_b64: String,
    note: String,
}

fn sha256(d: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(d);
    h.finalize().into()
}

/// Map arbitrary bytes to a canonical BN254 field element.
/// Clearing the top 3 bits of the (little-endian) most-significant byte guarantees the
/// value is < 2^253 < p, so `fr_from_bytes` is always `Some`.
fn fr_from_any(data: &[u8]) -> Fr {
    let mut b = sha256(data);
    b[31] &= 0x1f;
    fr_from_bytes(&b).expect("canonical fr")
}

/// Leaf = Poseidon(pk_hash, amount) — same gadget PRUV uses for committed leaves.
fn winner_leaf(e: &ClaimEntry) -> Fr {
    let pk = fr_from_any(format!("wallet:{}", e.wallet).as_bytes());
    let amt = fr_from_any(format!("amount:{}", e.amount).as_bytes());
    leaf_commitment(pk, amt)
}

/// Precomputed zero-subtree hashes for an empty depth-DEPTH tree.
fn zero_hashes() -> Vec<Fr> {
    let mut z = vec![Fr::from(0u64)];
    for i in 0..DEPTH {
        let p = z[i];
        z.push(hash_two(p, p));
    }
    z
}

/// Extract the depth-DEPTH Merkle path (siblings + bits) for `idx`, padding empty
/// subtrees with zero-hashes. Root is computed via `merkle_root_from_path`, the exact
/// function `merkle::prove` validates against — so the witness is always consistent.
fn merkle_path(leaves: &[Fr], idx: usize) -> (Fr, Vec<Fr>, Vec<bool>) {
    let zeros = zero_hashes();
    let mut cur: Vec<Fr> = leaves.to_vec();
    let mut index = idx;
    let mut siblings = Vec::with_capacity(DEPTH);
    let mut bits = Vec::with_capacity(DEPTH);
    for level in 0..DEPTH {
        if cur.len() % 2 == 1 {
            cur.push(zeros[level]);
        }
        let sib = index ^ 1;
        siblings.push(if sib < cur.len() { cur[sib] } else { zeros[level] });
        bits.push(index & 1 == 1);
        let mut next = Vec::with_capacity(cur.len() / 2);
        let mut i = 0;
        while i < cur.len() {
            next.push(hash_two(cur[i], cur[i + 1]));
            i += 2;
        }
        cur = next;
        index >>= 1;
    }
    let root = merkle_root_from_path(leaves[idx], &siblings, &bits);
    (root, siblings, bits)
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.spike {
        return topn::run_spike();
    }

    let raw = std::fs::read_to_string(&args.claims)
        .with_context(|| format!("read {}", args.claims.display()))?;
    let tree: ClaimTree = serde_json::from_str(&raw).context("parse claims.json")?;
    anyhow::ensure!(!tree.claims.is_empty(), "no claims in tree");
    anyhow::ensure!(args.index < tree.claims.len(), "index out of range");

    // stable leaf order = by claim index
    let mut winners = tree.claims.clone();
    winners.sort_by_key(|c| c.index);

    let leaves: Vec<Fr> = winners.iter().map(winner_leaf).collect();
    let (root, siblings, bits) = merkle_path(&leaves, args.index);

    let witness = MerkleWitness {
        leaf: fr_to_bytes(leaves[args.index]),
        siblings: siblings.iter().map(|s| fr_to_bytes(*s)).collect(),
        path_bits: bits,
        root: fr_to_bytes(root),
    };

    eprintln!(
        "proving inclusion of winner #{} of {} (depth {DEPTH}) …",
        args.index,
        leaves.len()
    );
    let proof = merkle::prove(&witness).context("halo2 prove")?;
    anyhow::ensure!(
        merkle::verify(&proof).context("halo2 verify")?,
        "self-verify failed"
    );

    let proof_hash = sha256(&proof.proof_bytes);
    let out = ProofOut {
        scheme: format!("poseidon-merkle-depth{DEPTH}-bn254-kzg"),
        index: args.index,
        wallet: winners[args.index].wallet.clone(),
        leaf: hex::encode(fr_to_bytes(leaves[args.index])),
        poseidon_root: hex::encode(fr_to_bytes(root)),
        proof_hash: hex::encode(proof_hash),
        public_inputs: proof.public_inputs.iter().map(hex::encode).collect(),
        proof_b64: base64::engine::general_purpose::STANDARD.encode(&proof.proof_bytes),
        note: "Phase 1: proves winner is in the committed Poseidon tree (parallel to the \
               SHA-256 claim tree). Phase 2 proves the allocation function itself."
            .into(),
    };

    std::fs::create_dir_all(&args.out)?;
    std::fs::write(
        args.out.join("allocation-proof.json"),
        serde_json::to_vec_pretty(&out)?,
    )?;
    std::fs::write(args.out.join("allocation-proof.bin"), &proof.proof_bytes)?;

    println!("✓ proof verified");
    println!("  poseidon_root : {}", out.poseidon_root);
    println!("  proof_hash    : {}", out.proof_hash);
    println!(
        "  proof_bytes   : {} B  ->  {}/allocation-proof.bin",
        proof.proof_bytes.len(),
        args.out.display()
    );
    Ok(())
}

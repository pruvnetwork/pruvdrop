//! Phase-2 **spike**: top-N counting/threshold **+ a sound input binding**.
//!
//! Proves, over M witnessed scores:
//!   1. **exactly N have score ≥ t** (bit-decomposition comparator + running count), and
//!   2. those scores are the ones committed in a public **fingerprint** `C = Σ scoreᵢ·rⁱ`
//!      under a public challenge `r` (a Horner accumulator constrained in-circuit).
//! Public (instance column): `[t, N, r, C]`.
//!
//! Why a fingerprint and not a Poseidon Merkle root? PRUV's `PoseidonChip` (used by
//! `merkle`/`governance`) assigns the hash output as a *free* advice cell with **no gate
//! enforcing `out == Poseidon(in_a,in_b)`** — so those circuits (and the Phase-1
//! inclusion proof) are only sound for an honest prover. A faithful Poseidon-Merkle
//! binding needs a real permutation gadget (SboxChip + MDS) wired in pruv-circuits.
//! Until then, this RLC fingerprint is a binding we can constrain **soundly** with our
//! own gates: under a random `r` two different score vectors collide on `C` only with
//! negligible probability (Schwartz–Zippel). In production `r` is a Fiat–Shamir challenge.
//!
//! Run:  cargo run --release -- --spike

use anyhow::{anyhow, Result};
use halo2_proofs::{
    circuit::{AssignedCell, Layouter, SimpleFloorPlanner, Value},
    halo2curves::bn256::{Fr, G1Affine},
    plonk::{
        Advice, Circuit, Column, ConstraintSystem, ErrorFront, Expression, Fixed, Instance,
        Selector,
    },
    poly::Rotation,
};

const B: usize = 16; // score / t bit-width
const OFFSET: u64 = 1 << B; // 2^B
const TOPN_K: u32 = 11;
const CHALLENGE: u64 = 2_718_281_829; // public RLC challenge r (Fiat-Shamir in production)

#[derive(Clone)]
pub struct TopNConfig {
    score: Column<Advice>,
    tcol: Column<Advice>,
    bits: Vec<Column<Advice>>, // B+1 bits
    win: Column<Advice>,
    acc: Column<Advice>,  // running count
    rcol: Column<Advice>, // public challenge r (per row)
    accc: Column<Advice>, // Horner fingerprint accumulator
    instance: Column<Instance>,
    constants: Column<Fixed>,
    q: Selector,
}

#[derive(Clone)]
pub struct TopNCircuit {
    pub scores: Vec<Value<u64>>,
    pub t: Value<u64>,
    pub r: Fr,
}

impl TopNCircuit {
    pub fn empty(m: usize) -> Self {
        Self { scores: vec![Value::unknown(); m], t: Value::unknown(), r: Fr::from(CHALLENGE) }
    }
}

impl Circuit<Fr> for TopNCircuit {
    type Config = TopNConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self { scores: vec![Value::unknown(); self.scores.len()], t: Value::unknown(), r: self.r }
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> TopNConfig {
        let score = meta.advice_column();
        let tcol = meta.advice_column();
        let bits: Vec<Column<Advice>> = (0..=B).map(|_| meta.advice_column()).collect();
        let win = meta.advice_column();
        let acc = meta.advice_column();
        let rcol = meta.advice_column();
        let accc = meta.advice_column();
        let instance = meta.instance_column();
        let constants = meta.fixed_column();
        let q = meta.selector();

        meta.enable_equality(tcol);
        meta.enable_equality(acc);
        meta.enable_equality(rcol);
        meta.enable_equality(accc);
        meta.enable_equality(instance);
        meta.enable_constant(constants);

        let bits_c = bits.clone();
        meta.create_gate("topn", |meta| {
            let q = meta.query_selector(q);
            let score = meta.query_advice(score, Rotation::cur());
            let tq = meta.query_advice(tcol, Rotation::cur());
            let win = meta.query_advice(win, Rotation::cur());
            let acc_cur = meta.query_advice(acc, Rotation::cur());
            let acc_next = meta.query_advice(acc, Rotation::next());
            let r = meta.query_advice(rcol, Rotation::cur());
            let accc_cur = meta.query_advice(accc, Rotation::cur());
            let accc_next = meta.query_advice(accc, Rotation::next());
            let bitq: Vec<Expression<Fr>> =
                bits_c.iter().map(|b| meta.query_advice(*b, Rotation::cur())).collect();

            let one = Expression::Constant(Fr::from(1u64));
            let mut cons: Vec<Expression<Fr>> = Vec::new();

            for b in &bitq {
                cons.push(q.clone() * b.clone() * (b.clone() - one.clone()));
            }
            cons.push(q.clone() * (win.clone() - bitq[B].clone()));
            let mut sum = Expression::Constant(Fr::from(0u64));
            for (k, b) in bitq.iter().enumerate() {
                sum = sum + b.clone() * Expression::Constant(Fr::from(1u64 << k));
            }
            let target = score.clone() - tq + Expression::Constant(Fr::from(OFFSET));
            cons.push(q.clone() * (sum - target));
            cons.push(q.clone() * (acc_next - acc_cur - win));
            // Horner fingerprint: accc_next == accc_cur * r + score
            cons.push(q.clone() * (accc_next - (accc_cur * r + score)));

            cons
        });

        TopNConfig { score, tcol, bits, win, acc, rcol, accc, instance, constants, q }
    }

    fn synthesize(&self, config: TopNConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.scores.len();
        let r = self.r;
        let (t_cells, r_cells, acc_final, accc_final) = layouter.assign_region(
            || "topn",
            |mut region| {
                let mut acc_val: Value<u64> = Value::known(0);
                let mut accc_val: Value<Fr> = Value::known(Fr::from(0u64));

                let acc0 = region.assign_advice(|| "acc0", config.acc, 0, || acc_val.map(Fr::from))?;
                region.constrain_constant(acc0.cell(), Fr::from(0u64))?;
                let accc0 = region.assign_advice(|| "accc0", config.accc, 0, || accc_val)?;
                region.constrain_constant(accc0.cell(), Fr::from(0u64))?;

                let mut t_cells: Vec<AssignedCell<Fr, Fr>> = Vec::with_capacity(m);
                let mut r_cells: Vec<AssignedCell<Fr, Fr>> = Vec::with_capacity(m);
                let mut acc_cell = acc0;
                let mut accc_cell = accc0;

                for i in 0..m {
                    config.q.enable(&mut region, i)?;
                    let score = self.scores[i];
                    let t = self.t;
                    let score_fr = score.map(Fr::from);

                    region.assign_advice(|| "score", config.score, i, || score_fr)?;
                    let tcell = region.assign_advice(|| "t", config.tcol, i, || t.map(Fr::from))?;
                    t_cells.push(tcell);
                    let rcell = region.assign_advice(|| "r", config.rcol, i, || Value::known(r))?;
                    r_cells.push(rcell);

                    let diff: Value<u64> =
                        score.zip(t).map(|(s, tt)| (s as i64 - tt as i64 + OFFSET as i64) as u64);
                    for k in 0..=B {
                        region.assign_advice(|| "bit", config.bits[k], i, || diff.map(|d| Fr::from((d >> k) & 1)))?;
                    }
                    let win_val: Value<u64> = diff.map(|d| (d >> B) & 1);
                    region.assign_advice(|| "win", config.win, i, || win_val.map(Fr::from))?;

                    acc_val = acc_val.zip(win_val).map(|(a, w)| a + w);
                    acc_cell = region.assign_advice(|| "acc", config.acc, i + 1, || acc_val.map(Fr::from))?;

                    accc_val = accc_val.zip(score_fr).map(|(a, s)| a * r + s);
                    accc_cell = region.assign_advice(|| "accc", config.accc, i + 1, || accc_val)?;
                }
                Ok((t_cells, r_cells, acc_cell, accc_cell))
            },
        )?;

        for tc in &t_cells {
            layouter.constrain_instance(tc.cell(), config.instance, 0)?; // t
        }
        for rc in &r_cells {
            layouter.constrain_instance(rc.cell(), config.instance, 2)?; // r
        }
        layouter.constrain_instance(acc_final.cell(), config.instance, 1)?; // N
        layouter.constrain_instance(accc_final.cell(), config.instance, 3)?; // C
        Ok(())
    }
}

// ─── prove / verify (own keygen) ────────────────────────────────────────────────

fn instance_vec(t: Fr, n: Fr, r: Fr, c: Fr) -> Vec<Vec<Vec<Fr>>> {
    vec![vec![vec![t, n, r, c]]]
}

pub fn prove(circuit: &TopNCircuit, t: Fr, n: Fr, r: Fr, c: Fr, m: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;

    let params = pruv_circuits::srs::get(TOPN_K)?;
    let empty = TopNCircuit::empty(m);
    let vk = keygen_vk(&*params, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;

    let instances = instance_vec(t, n, r, c);
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params,
        &pk,
        &[circuit.clone()],
        &instances,
        OsRng,
        &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], t: Fr, n: Fr, r: Fr, c: Fr, m: usize) -> Result<bool> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{keygen_vk, verify_proof_multi},
        poly::kzg::{
            commitment::{KZGCommitmentScheme, ParamsVerifierKZG},
            multiopen::VerifierGWC,
            strategy::SingleStrategy,
        },
        transcript::{Blake2bRead, Challenge255, TranscriptReadBuffer},
    };

    let params = pruv_circuits::srs::get(TOPN_K)?;
    let empty = TopNCircuit::empty(m);
    let vk = keygen_vk(&*params, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;

    let verifier_params: ParamsVerifierKZG<Bn256> = params.verifier_params().clone();
    let instances = instance_vec(t, n, r, c);
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    let ok = verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params,
        &vk,
        &instances,
        &mut transcript,
    );
    Ok(ok)
}

/// Native fingerprint C = Σ scoreᵢ·r^(M-1-i) (Horner), matching the in-circuit accumulator.
fn fingerprint(scores: &[u64], r: Fr) -> Fr {
    let mut c = Fr::from(0u64);
    for &s in scores {
        c = c * r + Fr::from(s);
    }
    c
}

pub fn run_spike() -> Result<()> {
    let scores: Vec<u64> = vec![120, 80, 200, 50, 300, 95, 40, 600];
    let t: u64 = 100;
    let m = scores.len();
    let r = Fr::from(CHALLENGE);
    let n = scores.iter().filter(|&&s| s >= t).count() as u64;
    let c = fingerprint(&scores, r);

    let circuit = TopNCircuit {
        scores: scores.iter().map(|&s| Value::known(s)).collect(),
        t: Value::known(t),
        r,
    };

    eprintln!("spike: proving Σ(score ≥ {t}) = {n} over M={m}, bound to fingerprint C …");
    let proof = prove(&circuit, Fr::from(t), Fr::from(n), r, c, m)?;
    anyhow::ensure!(verify(&proof, Fr::from(t), Fr::from(n), r, c, m)?, "verify failed");
    println!("✓ spike proof verified");
    println!("  statement   : exactly N={n} of M={m} scores ≥ t={t}, bound to committed C");
    println!("  proof_bytes : {} B  (k={TOPN_K})", proof.len());

    // soundness 1: false count must fail
    anyhow::ensure!(!verify(&proof, Fr::from(t), Fr::from(n + 1), r, c, m)?, "wrong N verified");
    println!("  negative (N+1)         : rejected ✓");

    // soundness 2: tamper a score (keep N the same) — must not match the committed C
    let mut tampered = scores.clone();
    tampered[1] += 1; // 80 -> 81, still < t, so N is unchanged (4)
    let bad_n = tampered.iter().filter(|&&s| s >= t).count() as u64;
    let bad_circuit = TopNCircuit {
        scores: tampered.iter().map(|&s| Value::known(s)).collect(),
        t: Value::known(t),
        r,
    };
    // prove the tampered scores but claim the ORIGINAL committed C
    let rejected = match prove(&bad_circuit, Fr::from(t), Fr::from(bad_n), r, c, m) {
        Ok(p) => !verify(&p, Fr::from(t), Fr::from(bad_n), r, c, m)?,
        Err(_) => true,
    };
    anyhow::ensure!(rejected, "binding broken: tampered scores matched C");
    println!("  binding (swap a score) : rejected ✓  (scores are bound to C)");
    Ok(())
}

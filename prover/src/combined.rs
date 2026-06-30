//! Combined Phase-2 core: **sound counting over a Poseidon-committed set**.
//!
//! Proves in one circuit:
//!   1. `C = Poseidon-chain(scores)` — a sound Poseidon commitment to the score vector
//!      (`C₀=0`, `C_{i+1}=Poseidon(C_i, score_i)`), each step the constrained permutation;
//!      this replaces the RLC fingerprint in `topn.rs` with a real Poseidon binding.
//!   2. **exactly N of those same scores are ≥ t** (bit-decomposition comparator + count).
//! The score cells are shared between (1) and (2) by copy constraints, so the count is
//! provably over the committed set. Public (instance): `[t, N, C]`.
//!
//! Run:  cargo run --release -- --combined

use anyhow::{anyhow, ensure, Result};
use halo2_proofs::{
    circuit::{AssignedCell, Layouter, SimpleFloorPlanner, Value},
    halo2curves::bn256::{Fr, G1Affine},
    plonk::{
        Advice, Circuit, Column, ConstraintSystem, ErrorFront, Expression, Fixed, Instance,
        Selector,
    },
    poly::Rotation,
};

use crate::poseidon::{params, perm_native, perm_trace};

const B: usize = 16;
const OFFSET: u64 = 1 << B;
const K: u32 = 12;

#[derive(Clone)]
pub struct CombinedConfig {
    score: Column<Advice>,
    tcol: Column<Advice>,
    bits: Vec<Column<Advice>>,
    win: Column<Advice>,
    acc: Column<Advice>,
    s: [Column<Advice>; 3],
    ark: [Column<Fixed>; 3],
    mds: [[Column<Fixed>; 3]; 3],
    constants: Column<Fixed>,
    q_count: Selector,
    q_full: Selector,
    q_part: Selector,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct CombinedCircuit {
    pub scores: Vec<Value<u64>>,
    pub t: Value<u64>,
}

impl CombinedCircuit {
    pub fn empty(m: usize) -> Self {
        Self { scores: vec![Value::unknown(); m], t: Value::unknown() }
    }
}

impl Circuit<Fr> for CombinedCircuit {
    type Config = CombinedConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::empty(self.scores.len())
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> CombinedConfig {
        let score = meta.advice_column();
        let tcol = meta.advice_column();
        let bits: Vec<Column<Advice>> = (0..=B).map(|_| meta.advice_column()).collect();
        let win = meta.advice_column();
        let acc = meta.advice_column();
        let s = [meta.advice_column(), meta.advice_column(), meta.advice_column()];
        let ark = [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()];
        let mds = [
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
        ];
        let constants = meta.fixed_column();
        let instance = meta.instance_column();
        let q_count = meta.selector();
        let q_full = meta.selector();
        let q_part = meta.selector();

        for c in [score, tcol, acc, s[0], s[1], s[2]] {
            meta.enable_equality(c);
        }
        meta.enable_equality(instance);
        meta.enable_constant(constants);

        let bits_c = bits.clone();
        meta.create_gate("count", |meta| {
            let q = meta.query_selector(q_count);
            let score = meta.query_advice(score, Rotation::cur());
            let tq = meta.query_advice(tcol, Rotation::cur());
            let win = meta.query_advice(win, Rotation::cur());
            let acc_cur = meta.query_advice(acc, Rotation::cur());
            let acc_next = meta.query_advice(acc, Rotation::next());
            let bitq: Vec<Expression<Fr>> =
                bits_c.iter().map(|b| meta.query_advice(*b, Rotation::cur())).collect();
            let one = Expression::Constant(Fr::from(1u64));
            let mut cons = Vec::new();
            for b in &bitq {
                cons.push(q.clone() * b.clone() * (b.clone() - one.clone()));
            }
            cons.push(q.clone() * (win.clone() - bitq[B].clone()));
            let mut sum = Expression::Constant(Fr::from(0u64));
            for (k, b) in bitq.iter().enumerate() {
                sum = sum + b.clone() * Expression::Constant(Fr::from(1u64 << k));
            }
            cons.push(q.clone() * (sum - (score - tq + Expression::Constant(Fr::from(OFFSET)))));
            cons.push(q.clone() * (acc_next - acc_cur - win));
            cons
        });

        let pow5e = |e: Expression<Fr>| {
            let e2 = e.clone() * e.clone();
            let e4 = e2.clone() * e2.clone();
            e4 * e
        };
        meta.create_gate("full_round", |meta| {
            let q = meta.query_selector(q_full);
            let s_cur: Vec<_> = (0..3).map(|i| meta.query_advice(s[i], Rotation::cur())).collect();
            let s_next: Vec<_> = (0..3).map(|i| meta.query_advice(s[i], Rotation::next())).collect();
            let ark_q: Vec<_> = (0..3).map(|i| meta.query_fixed(ark[i], Rotation::cur())).collect();
            let mds_q: Vec<Vec<_>> = (0..3)
                .map(|i| (0..3).map(|j| meta.query_fixed(mds[i][j], Rotation::cur())).collect())
                .collect();
            let sb: Vec<_> = (0..3).map(|j| pow5e(s_cur[j].clone() + ark_q[j].clone())).collect();
            (0..3)
                .map(|i| {
                    let rhs = (0..3).fold(Expression::Constant(Fr::from(0u64)), |acc, j| {
                        acc + sb[j].clone() * mds_q[i][j].clone()
                    });
                    q.clone() * (s_next[i].clone() - rhs)
                })
                .collect::<Vec<_>>()
        });
        meta.create_gate("partial_round", |meta| {
            let q = meta.query_selector(q_part);
            let s_cur: Vec<_> = (0..3).map(|i| meta.query_advice(s[i], Rotation::cur())).collect();
            let s_next: Vec<_> = (0..3).map(|i| meta.query_advice(s[i], Rotation::next())).collect();
            let ark_q: Vec<_> = (0..3).map(|i| meta.query_fixed(ark[i], Rotation::cur())).collect();
            let mds_q: Vec<Vec<_>> = (0..3)
                .map(|i| (0..3).map(|j| meta.query_fixed(mds[i][j], Rotation::cur())).collect())
                .collect();
            let sb: Vec<Expression<Fr>> = vec![
                pow5e(s_cur[0].clone() + ark_q[0].clone()),
                s_cur[1].clone() + ark_q[1].clone(),
                s_cur[2].clone() + ark_q[2].clone(),
            ];
            (0..3)
                .map(|i| {
                    let rhs = (0..3).fold(Expression::Constant(Fr::from(0u64)), |acc, j| {
                        acc + sb[j].clone() * mds_q[i][j].clone()
                    });
                    q.clone() * (s_next[i].clone() - rhs)
                })
                .collect::<Vec<_>>()
        });

        CombinedConfig { score, tcol, bits, win, acc, s, ark, mds, constants, q_count, q_full, q_part, instance }
    }

    fn synthesize(&self, config: CombinedConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.scores.len();
        let p = params();
        let half = p.full_rounds / 2;
        let all = p.full_rounds + p.partial_rounds;

        // ── counting region (also yields the canonical score cells) ──
        let (score_cells, t_cells, acc_final) = layouter.assign_region(
            || "count",
            |mut region| {
                let mut acc_val: Value<u64> = Value::known(0);
                let acc0 = region.assign_advice(|| "acc0", config.acc, 0, || acc_val.map(Fr::from))?;
                region.constrain_constant(acc0.cell(), Fr::from(0u64))?;
                let mut score_cells = Vec::with_capacity(m);
                let mut t_cells = Vec::with_capacity(m);
                let mut acc_cell = acc0;
                for i in 0..m {
                    config.q_count.enable(&mut region, i)?;
                    let score = self.scores[i];
                    let t = self.t;
                    let sc = region.assign_advice(|| "score", config.score, i, || score.map(Fr::from))?;
                    score_cells.push(sc);
                    t_cells.push(region.assign_advice(|| "t", config.tcol, i, || t.map(Fr::from))?);
                    let diff: Value<u64> =
                        score.zip(t).map(|(s, tt)| (s as i64 - tt as i64 + OFFSET as i64) as u64);
                    for k in 0..=B {
                        region.assign_advice(|| "bit", config.bits[k], i, || diff.map(|d| Fr::from((d >> k) & 1)))?;
                    }
                    let win_val: Value<u64> = diff.map(|d| (d >> B) & 1);
                    region.assign_advice(|| "win", config.win, i, || win_val.map(Fr::from))?;
                    acc_val = acc_val.zip(win_val).map(|(a, w)| a + w);
                    acc_cell = region.assign_advice(|| "acc", config.acc, i + 1, || acc_val.map(Fr::from))?;
                }
                Ok((score_cells, t_cells, acc_cell))
            },
        )?;

        // ── Poseidon commitment chain: C_{i+1} = Poseidon(C_i, score_i) ──
        let mut cur: Option<AssignedCell<Fr, Fr>> = None;
        for i in 0..m {
            let score_val = self.scores[i].map(Fr::from);
            let cin_val: Value<Fr> = match &cur {
                None => Value::known(Fr::from(0u64)),
                Some(c) => c.value().copied(),
            };
            let trace = cin_val.zip(score_val).map(|(c, s)| perm_trace(c, s));
            let prev = cur.clone();
            let score_cell = score_cells[i].clone();

            let parent = layouter.assign_region(
                || format!("chain_{i}"),
                |mut region| {
                    for r in 0..all {
                        for k in 0..3 {
                            region.assign_fixed(|| "ark", config.ark[k], r, || Value::known(p.ark[r * 3 + k]))?;
                        }
                        for a in 0..3 {
                            for b in 0..3 {
                                region.assign_fixed(|| "mds", config.mds[a][b], r, || Value::known(p.mds[a][b]))?;
                            }
                        }
                        let full = r < half || r >= half + p.partial_rounds;
                        if full {
                            config.q_full.enable(&mut region, r)?;
                        } else {
                            config.q_part.enable(&mut region, r)?;
                        }
                    }
                    // row 0 inputs: s0=0, s1=C_i, s2=score_i
                    let s0 = region.assign_advice(|| "s0", config.s[0], 0, || Value::known(Fr::from(0u64)))?;
                    region.constrain_constant(s0.cell(), Fr::from(0u64))?;
                    match &prev {
                        None => {
                            let c = region.assign_advice(|| "C0", config.s[1], 0, || Value::known(Fr::from(0u64)))?;
                            region.constrain_constant(c.cell(), Fr::from(0u64))?;
                        }
                        Some(pc) => {
                            pc.copy_advice(|| "C_i", &mut region, config.s[1], 0)?;
                        }
                    }
                    score_cell.copy_advice(|| "score_i", &mut region, config.s[2], 0)?;
                    // rows 1..=all from the permutation trace
                    let mut parent = None;
                    for r in 1..=all {
                        for k in 0..3 {
                            let v = trace.clone().map(|t| t[r][k]);
                            let cell = region.assign_advice(|| "s", config.s[k], r, || v)?;
                            if r == all && k == 0 {
                                parent = Some(cell);
                            }
                        }
                    }
                    Ok(parent.unwrap())
                },
            )?;
            cur = Some(parent);
        }

        for tc in &t_cells {
            layouter.constrain_instance(tc.cell(), config.instance, 0)?; // t
        }
        layouter.constrain_instance(acc_final.cell(), config.instance, 1)?; // N
        layouter.constrain_instance(cur.unwrap().cell(), config.instance, 2)?; // C
        Ok(())
    }
}

fn chain_native(scores: &[u64]) -> Fr {
    let mut c = Fr::from(0u64);
    for &s in scores {
        c = perm_native(c, Fr::from(s));
    }
    c
}

// ─── prove / verify ─────────────────────────────────────────────────────────────

pub fn prove(circuit: &CombinedCircuit, t: Fr, n: Fr, c: Fr, m: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;

    let params_kzg = pruv_circuits::srs::get(K)?;
    let empty = CombinedCircuit::empty(m);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params_kzg, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![t, n, c]]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params_kzg, &pk, &[circuit.clone()], instances, OsRng, &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], t: Fr, n: Fr, c: Fr, m: usize) -> Result<bool> {
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
    let params_kzg = pruv_circuits::srs::get(K)?;
    let empty = CombinedCircuit::empty(m);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params_kzg.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![t, n, c]]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params, &vk, instances, &mut transcript,
    ))
}

pub fn run_combined() -> Result<()> {
    let scores: Vec<u64> = vec![120, 80, 200, 50];
    let t: u64 = 100;
    let m = scores.len();
    let n = scores.iter().filter(|&&s| s >= t).count() as u64;
    let c = chain_native(&scores);

    let circuit = CombinedCircuit {
        scores: scores.iter().map(|&s| Value::known(s)).collect(),
        t: Value::known(t),
    };

    eprintln!("proving: Σ(score ≥ {t}) = {n} over M={m}, bound to Poseidon commitment C …");
    let proof = prove(&circuit, Fr::from(t), Fr::from(n), c, m)?;
    ensure!(verify(&proof, Fr::from(t), Fr::from(n), c, m)?, "verify failed");
    println!("✓ combined proof verified — sound counting over a Poseidon-committed set");
    println!("  N={n} of M={m} scores ≥ t={t}, bound to a Poseidon-chain commitment");
    println!("  proof_bytes = {} B  (k={K})", proof.len());

    ensure!(!verify(&proof, Fr::from(t), Fr::from(n + 1), c, m)?, "wrong N verified");
    println!("  negative (N+1)          : rejected ✓");

    // tamper a (losing) score, keep the committed C → must fail the Poseidon binding
    let mut bad = scores.clone();
    bad[1] += 1; // 80 -> 81, N unchanged
    let bad_circuit = CombinedCircuit {
        scores: bad.iter().map(|&s| Value::known(s)).collect(),
        t: Value::known(t),
    };
    let rejected = match prove(&bad_circuit, Fr::from(t), Fr::from(n), c, m) {
        Ok(pf) => !verify(&pf, Fr::from(t), Fr::from(n), c, m)?,
        Err(_) => true,
    };
    ensure!(rejected, "Poseidon binding broken: tampered scores matched C");
    println!("  binding (swap a score)  : rejected ✓  (Poseidon commitment)");
    Ok(())
}

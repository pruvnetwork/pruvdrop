//! Phase-2 payout: real amounts with sound conservation.
//!
//! Extends selection with per-winner payout amounts. Proves:
//!   1. `inputC` commits `(wallet_i, score_i)` per candidate (sound Poseidon chain).
//!   2. `win_i = (score_i ≥ t)`, `Σ win_i = N`.
//!   3. **losers get exactly 0**: `amount_i · (1 − win_i) = 0`.
//!   4. **Σ amount_i = pot** (public) — conservation, no inflation/loss.
//!   5. `claimC` commits `(wallet_i, amount_i)` per candidate (the claim tree contents).
//! Public (instance): `[t, N, pot, inputC, claimC]`.
//!
//! The amount *values* are the operator's published split (e.g. ∝ score, computed
//! off-chain and re-checkable from the public scores); the circuit soundly enforces the
//! invariants (masking + conservation + identity binding). In-circuit proportional
//! rounding is the documented next refinement.
//!
//! Run:  cargo run --release -- --payout

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
const K: u32 = 14;

#[derive(Clone)]
pub struct PayConfig {
    score: Column<Advice>,
    wallet: Column<Advice>,
    tcol: Column<Advice>,
    bits: Vec<Column<Advice>>,
    win: Column<Advice>,
    amount: Column<Advice>,
    acc: Column<Advice>,     // running winner count
    acc_amt: Column<Advice>, // running amount sum
    s: [Column<Advice>; 3],
    ark: [Column<Fixed>; 3],
    mds: [[Column<Fixed>; 3]; 3],
    constants: Column<Fixed>,
    q_count: Selector,
    q_mask: Selector,
    q_full: Selector,
    q_part: Selector,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct PayCircuit {
    pub wallets: Vec<Value<Fr>>,
    pub scores: Vec<Value<u64>>,
    pub amounts: Vec<Value<Fr>>,
    pub t: Value<u64>,
}

impl PayCircuit {
    pub fn empty(m: usize) -> Self {
        Self {
            wallets: vec![Value::unknown(); m],
            scores: vec![Value::unknown(); m],
            amounts: vec![Value::unknown(); m],
            t: Value::unknown(),
        }
    }
}

enum In {
    Zero,
    Cell(AssignedCell<Fr, Fr>),
}
fn lval(i: &In) -> Value<Fr> {
    match i {
        In::Zero => Value::known(Fr::from(0u64)),
        In::Cell(c) => c.value().copied(),
    }
}

fn hash2(
    layouter: &mut impl Layouter<Fr>,
    cfg: &PayConfig,
    left: &In,
    right: &AssignedCell<Fr, Fr>,
) -> Result<AssignedCell<Fr, Fr>, ErrorFront> {
    let p = params();
    let half = p.full_rounds / 2;
    let all = p.full_rounds + p.partial_rounds;
    let trace = lval(left).zip(right.value().copied()).map(|(l, r)| perm_trace(l, r));
    layouter.assign_region(
        || "hash2",
        |mut region| {
            for r in 0..all {
                for k in 0..3 {
                    region.assign_fixed(|| "ark", cfg.ark[k], r, || Value::known(p.ark[r * 3 + k]))?;
                }
                for a in 0..3 {
                    for b in 0..3 {
                        region.assign_fixed(|| "mds", cfg.mds[a][b], r, || Value::known(p.mds[a][b]))?;
                    }
                }
                let full = r < half || r >= half + p.partial_rounds;
                if full {
                    cfg.q_full.enable(&mut region, r)?;
                } else {
                    cfg.q_part.enable(&mut region, r)?;
                }
            }
            let s0 = region.assign_advice(|| "s0", cfg.s[0], 0, || Value::known(Fr::from(0u64)))?;
            region.constrain_constant(s0.cell(), Fr::from(0u64))?;
            match left {
                In::Zero => {
                    let c = region.assign_advice(|| "L0", cfg.s[1], 0, || Value::known(Fr::from(0u64)))?;
                    region.constrain_constant(c.cell(), Fr::from(0u64))?;
                }
                In::Cell(c) => {
                    c.copy_advice(|| "L", &mut region, cfg.s[1], 0)?;
                }
            }
            right.copy_advice(|| "R", &mut region, cfg.s[2], 0)?;
            let mut parent = None;
            for r in 1..=all {
                for k in 0..3 {
                    let v = trace.clone().map(|t| t[r][k]);
                    let cell = region.assign_advice(|| "s", cfg.s[k], r, || v)?;
                    if r == all && k == 0 {
                        parent = Some(cell);
                    }
                }
            }
            Ok(parent.unwrap())
        },
    )
}

impl Circuit<Fr> for PayCircuit {
    type Config = PayConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::empty(self.scores.len())
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> PayConfig {
        let score = meta.advice_column();
        let wallet = meta.advice_column();
        let tcol = meta.advice_column();
        let bits: Vec<Column<Advice>> = (0..=B).map(|_| meta.advice_column()).collect();
        let win = meta.advice_column();
        let amount = meta.advice_column();
        let acc = meta.advice_column();
        let acc_amt = meta.advice_column();
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
        let q_mask = meta.selector();
        let q_full = meta.selector();
        let q_part = meta.selector();

        for c in [score, wallet, tcol, amount, acc, acc_amt, s[0], s[1], s[2]] {
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

        meta.create_gate("mask", |meta| {
            let q = meta.query_selector(q_mask);
            let win = meta.query_advice(win, Rotation::cur());
            let amount = meta.query_advice(amount, Rotation::cur());
            let aa_cur = meta.query_advice(acc_amt, Rotation::cur());
            let aa_next = meta.query_advice(acc_amt, Rotation::next());
            let one = Expression::Constant(Fr::from(1u64));
            vec![
                q.clone() * amount.clone() * (one - win),   // loser → amount 0
                q * (aa_next - aa_cur - amount),            // running amount sum
            ]
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

        PayConfig {
            score, wallet, tcol, bits, win, amount, acc, acc_amt, s, ark, mds, constants,
            q_count, q_mask, q_full, q_part, instance,
        }
    }

    fn synthesize(&self, config: PayConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.scores.len();

        let (score_cells, wallet_cells, amount_cells, t_cells, acc_final, acc_amt_final) = layouter
            .assign_region(
                || "count_mask",
                |mut region| {
                    let mut acc_val: Value<u64> = Value::known(0);
                    let mut aa_val: Value<Fr> = Value::known(Fr::from(0u64));
                    let acc0 = region.assign_advice(|| "acc0", config.acc, 0, || acc_val.map(Fr::from))?;
                    region.constrain_constant(acc0.cell(), Fr::from(0u64))?;
                    let aa0 = region.assign_advice(|| "aa0", config.acc_amt, 0, || aa_val)?;
                    region.constrain_constant(aa0.cell(), Fr::from(0u64))?;
                    let mut score_cells = Vec::with_capacity(m);
                    let mut wallet_cells = Vec::with_capacity(m);
                    let mut amount_cells = Vec::with_capacity(m);
                    let mut t_cells = Vec::with_capacity(m);
                    let mut acc_cell = acc0;
                    let mut aa_cell = aa0;
                    for i in 0..m {
                        config.q_count.enable(&mut region, i)?;
                        config.q_mask.enable(&mut region, i)?;
                        let score = self.scores[i];
                        let wallet = self.wallets[i];
                        let amount = self.amounts[i];
                        let t = self.t;
                        let sc = region.assign_advice(|| "score", config.score, i, || score.map(Fr::from))?;
                        let wc = region.assign_advice(|| "wallet", config.wallet, i, || wallet)?;
                        let amc = region.assign_advice(|| "amount", config.amount, i, || amount)?;
                        t_cells.push(region.assign_advice(|| "t", config.tcol, i, || t.map(Fr::from))?);
                        let diff: Value<u64> =
                            score.zip(t).map(|(s, tt)| (s as i64 - tt as i64 + OFFSET as i64) as u64);
                        for k in 0..=B {
                            region.assign_advice(|| "bit", config.bits[k], i, || diff.map(|d| Fr::from((d >> k) & 1)))?;
                        }
                        let win_val: Value<u64> = diff.map(|d| (d >> B) & 1);
                        region.assign_advice(|| "win", config.win, i, || win_val.map(Fr::from))?;
                        score_cells.push(sc);
                        wallet_cells.push(wc);
                        amount_cells.push(amc);
                        acc_val = acc_val.zip(win_val).map(|(a, w)| a + w);
                        acc_cell = region.assign_advice(|| "acc", config.acc, i + 1, || acc_val.map(Fr::from))?;
                        aa_val = aa_val.zip(amount).map(|(a, am)| a + am);
                        aa_cell = region.assign_advice(|| "acc_amt", config.acc_amt, i + 1, || aa_val)?;
                    }
                    Ok((score_cells, wallet_cells, amount_cells, t_cells, acc_cell, aa_cell))
                },
            )?;

        // inputC = chain absorbing wallet_i, score_i
        let mut c = In::Zero;
        for i in 0..m {
            let cell = hash2(&mut layouter, &config, &c, &wallet_cells[i])?;
            c = In::Cell(cell);
            let cell = hash2(&mut layouter, &config, &c, &score_cells[i])?;
            c = In::Cell(cell);
        }
        let input_c = match c { In::Cell(cell) => cell, In::Zero => unreachable!() };

        // claimC = chain absorbing wallet_i, amount_i (the claim-tree contents)
        let mut cc = In::Zero;
        for i in 0..m {
            let cell = hash2(&mut layouter, &config, &cc, &wallet_cells[i])?;
            cc = In::Cell(cell);
            let cell = hash2(&mut layouter, &config, &cc, &amount_cells[i])?;
            cc = In::Cell(cell);
        }
        let claim_c = match cc { In::Cell(cell) => cell, In::Zero => unreachable!() };

        for tc in &t_cells {
            layouter.constrain_instance(tc.cell(), config.instance, 0)?;
        }
        layouter.constrain_instance(acc_final.cell(), config.instance, 1)?; // N
        layouter.constrain_instance(acc_amt_final.cell(), config.instance, 2)?; // pot
        layouter.constrain_instance(input_c.cell(), config.instance, 3)?; // inputC
        layouter.constrain_instance(claim_c.cell(), config.instance, 4)?; // claimC
        Ok(())
    }
}

fn native_input_c(wallets: &[Fr], scores: &[u64]) -> Fr {
    let mut c = Fr::from(0u64);
    for (w, s) in wallets.iter().zip(scores.iter()) {
        c = perm_native(c, *w);
        c = perm_native(c, Fr::from(*s));
    }
    c
}
fn native_claim_c(wallets: &[Fr], amounts: &[Fr]) -> Fr {
    let mut c = Fr::from(0u64);
    for (w, a) in wallets.iter().zip(amounts.iter()) {
        c = perm_native(c, *w);
        c = perm_native(c, *a);
    }
    c
}

pub fn prove(circuit: &PayCircuit, pubs: [Fr; 5], m: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;
    let params_kzg = pruv_circuits::srs::get(K)?;
    let empty = PayCircuit::empty(m);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params_kzg, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;
    let instances: &[Vec<Vec<Fr>>] = &[vec![pubs.to_vec()]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params_kzg, &pk, &[circuit.clone()], instances, OsRng, &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], pubs: [Fr; 5], m: usize) -> Result<bool> {
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
    let empty = PayCircuit::empty(m);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params_kzg.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![pubs.to_vec()]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params, &vk, instances, &mut transcript,
    ))
}

pub fn run_payout() -> Result<()> {
    let wallets: Vec<Fr> = vec![Fr::from(111u64), Fr::from(222u64), Fr::from(333u64)];
    let scores: Vec<u64> = vec![120, 80, 200];
    let t: u64 = 100;
    let pot: u64 = 10_000;
    let m = scores.len();
    let n = scores.iter().filter(|&&s| s >= t).count() as u64;

    // proportional split among winners (computed off-chain), dust to the first winner
    let s_sum: u64 = scores.iter().filter(|&&s| s >= t).sum();
    let mut amounts_u: Vec<u64> = scores
        .iter()
        .map(|&s| if s >= t { pot * s / s_sum } else { 0 })
        .collect();
    let dist: u64 = amounts_u.iter().sum();
    if let Some(first) = amounts_u.iter_mut().find(|a| **a > 0) {
        *first += pot - dist; // dust
    }
    let amounts: Vec<Fr> = amounts_u.iter().map(|a| Fr::from(*a)).collect();

    let ic = native_input_c(&wallets, &scores);
    let cc = native_claim_c(&wallets, &amounts);
    let pubs = [Fr::from(t), Fr::from(n), Fr::from(pot), ic, cc];

    let circuit = PayCircuit {
        wallets: wallets.iter().map(|w| Value::known(*w)).collect(),
        scores: scores.iter().map(|&s| Value::known(s)).collect(),
        amounts: amounts.iter().map(|a| Value::known(*a)).collect(),
        t: Value::known(t),
    };

    eprintln!("proving payout: {n} winners share pot={pot}, losers get 0, Σ=pot …");
    let proof = prove(&circuit, pubs, m)?;
    ensure!(verify(&proof, pubs, m)?, "verify failed");
    println!("✓ payout proof verified — real amounts, losers 0, Σ amount = pot");
    println!("  amounts = {amounts_u:?}  (Σ = {pot}), claimC commits (wallet, amount)");
    println!("  proof_bytes = {} B  (k={K})", proof.len());

    // negative: claim a wrong pot
    let mut wrong = pubs;
    wrong[2] = Fr::from(pot + 1);
    ensure!(!verify(&proof, wrong, m)?, "wrong pot verified");
    println!("  negative (Σ ≠ pot)        : rejected ✓");

    // negative: pay a loser (amount on a losing index) — mask gate must fail
    let mut bad_amounts = amounts.clone();
    bad_amounts[1] = Fr::from(500u64); // index 1 is a loser
    let bad = PayCircuit {
        wallets: wallets.iter().map(|w| Value::known(*w)).collect(),
        scores: scores.iter().map(|&s| Value::known(s)).collect(),
        amounts: bad_amounts.iter().map(|a| Value::known(*a)).collect(),
        t: Value::known(t),
    };
    let cc_bad = native_claim_c(&wallets, &bad_amounts);
    let pubs_bad = [Fr::from(t), Fr::from(n), Fr::from(pot + 500), ic, cc_bad];
    let rejected = match prove(&bad, pubs_bad, m) {
        Ok(pf) => !verify(&pf, pubs_bad, m)?,
        Err(_) => true,
    };
    ensure!(rejected, "mask broken: a loser was paid");
    println!("  negative (pay a loser)    : rejected ✓");
    Ok(())
}

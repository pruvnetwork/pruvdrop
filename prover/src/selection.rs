//! Phase-2 selection: prove the winners are **exactly the top-N**, bound to identity.
//!
//! For M candidates `(wallet_i, score_i)`:
//!   1. `inputC` = Poseidon chain absorbing `wallet_0, score_0, wallet_1, score_1, …`
//!      — a sound commitment to the candidate set (identities + scores).
//!   2. comparator gives `win_i = (score_i ≥ t)`; `Σ win_i = N` (public). Since `win_i`
//!      is sound and exactly N pass a single threshold `t`, the winners ARE the N highest.
//!   3. `selected_i = win_i · wallet_i` (winner's wallet, else 0); `claimC` = Poseidon
//!      chain over `selected_i` — a sound commitment to the winner set by identity.
//! Public (instance): `[t, N, inputC, claimC]`. Tamper any score/wallet → commitment fails.
//!
//! Run:  cargo run --release -- --selection

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
const K: u32 = 13;

#[derive(Clone)]
pub struct SelConfig {
    score: Column<Advice>,
    wallet: Column<Advice>,
    tcol: Column<Advice>,
    bits: Vec<Column<Advice>>,
    win: Column<Advice>,
    selected: Column<Advice>,
    acc: Column<Advice>,
    s: [Column<Advice>; 3],
    ark: [Column<Fixed>; 3],
    mds: [[Column<Fixed>; 3]; 3],
    constants: Column<Fixed>,
    q_count: Selector,
    q_sel: Selector,
    q_full: Selector,
    q_part: Selector,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct SelCircuit {
    pub wallets: Vec<Value<Fr>>,
    pub scores: Vec<Value<u64>>,
    pub t: Value<u64>,
}

impl SelCircuit {
    pub fn empty(m: usize) -> Self {
        Self { wallets: vec![Value::unknown(); m], scores: vec![Value::unknown(); m], t: Value::unknown() }
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

/// One sound Poseidon chain step: returns a cell holding Poseidon(left, right).
fn hash2(
    layouter: &mut impl Layouter<Fr>,
    cfg: &SelConfig,
    left: &In,
    right: &AssignedCell<Fr, Fr>,
) -> Result<AssignedCell<Fr, Fr>, ErrorFront> {
    let p = params();
    let half = p.full_rounds / 2;
    let all = p.full_rounds + p.partial_rounds;
    let lv = lval(left);
    let rv = right.value().copied();
    let trace = lv.zip(rv).map(|(l, r)| perm_trace(l, r));

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

impl Circuit<Fr> for SelCircuit {
    type Config = SelConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::empty(self.scores.len())
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> SelConfig {
        let score = meta.advice_column();
        let wallet = meta.advice_column();
        let tcol = meta.advice_column();
        let bits: Vec<Column<Advice>> = (0..=B).map(|_| meta.advice_column()).collect();
        let win = meta.advice_column();
        let selected = meta.advice_column();
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
        let q_sel = meta.selector();
        let q_full = meta.selector();
        let q_part = meta.selector();

        for c in [score, wallet, tcol, selected, acc, s[0], s[1], s[2]] {
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

        meta.create_gate("select", |meta| {
            let q = meta.query_selector(q_sel);
            let win = meta.query_advice(win, Rotation::cur());
            let wallet = meta.query_advice(wallet, Rotation::cur());
            let selected = meta.query_advice(selected, Rotation::cur());
            vec![q * (selected - win * wallet)] // selected = win · wallet
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

        SelConfig {
            score, wallet, tcol, bits, win, selected, acc, s, ark, mds, constants,
            q_count, q_sel, q_full, q_part, instance,
        }
    }

    fn synthesize(&self, config: SelConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.scores.len();

        let (score_cells, wallet_cells, selected_cells, t_cells, acc_final) = layouter.assign_region(
            || "count_select",
            |mut region| {
                let mut acc_val: Value<u64> = Value::known(0);
                let acc0 = region.assign_advice(|| "acc0", config.acc, 0, || acc_val.map(Fr::from))?;
                region.constrain_constant(acc0.cell(), Fr::from(0u64))?;
                let mut score_cells = Vec::with_capacity(m);
                let mut wallet_cells = Vec::with_capacity(m);
                let mut selected_cells = Vec::with_capacity(m);
                let mut t_cells = Vec::with_capacity(m);
                let mut acc_cell = acc0;
                for i in 0..m {
                    config.q_count.enable(&mut region, i)?;
                    config.q_sel.enable(&mut region, i)?;
                    let score = self.scores[i];
                    let wallet = self.wallets[i];
                    let t = self.t;
                    let sc = region.assign_advice(|| "score", config.score, i, || score.map(Fr::from))?;
                    let wc = region.assign_advice(|| "wallet", config.wallet, i, || wallet)?;
                    t_cells.push(region.assign_advice(|| "t", config.tcol, i, || t.map(Fr::from))?);
                    let diff: Value<u64> =
                        score.zip(t).map(|(s, tt)| (s as i64 - tt as i64 + OFFSET as i64) as u64);
                    for k in 0..=B {
                        region.assign_advice(|| "bit", config.bits[k], i, || diff.map(|d| Fr::from((d >> k) & 1)))?;
                    }
                    let win_val: Value<u64> = diff.map(|d| (d >> B) & 1);
                    region.assign_advice(|| "win", config.win, i, || win_val.map(Fr::from))?;
                    // selected = win · wallet
                    let sel_val = win_val.map(Fr::from).zip(wallet).map(|(w, wl)| w * wl);
                    let selc = region.assign_advice(|| "selected", config.selected, i, || sel_val)?;
                    score_cells.push(sc);
                    wallet_cells.push(wc);
                    selected_cells.push(selc);
                    acc_val = acc_val.zip(win_val).map(|(a, w)| a + w);
                    acc_cell = region.assign_advice(|| "acc", config.acc, i + 1, || acc_val.map(Fr::from))?;
                }
                Ok((score_cells, wallet_cells, selected_cells, t_cells, acc_cell))
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
        let input_c = match c {
            In::Cell(cell) => cell,
            In::Zero => unreachable!(),
        };

        // claimC = chain over selected_i
        let mut cc = In::Zero;
        for i in 0..m {
            let cell = hash2(&mut layouter, &config, &cc, &selected_cells[i])?;
            cc = In::Cell(cell);
        }
        let claim_c = match cc {
            In::Cell(cell) => cell,
            In::Zero => unreachable!(),
        };

        for tc in &t_cells {
            layouter.constrain_instance(tc.cell(), config.instance, 0)?;
        }
        layouter.constrain_instance(acc_final.cell(), config.instance, 1)?;
        layouter.constrain_instance(input_c.cell(), config.instance, 2)?;
        layouter.constrain_instance(claim_c.cell(), config.instance, 3)?;
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
fn native_claim_c(selected: &[Fr]) -> Fr {
    let mut c = Fr::from(0u64);
    for s in selected {
        c = perm_native(c, *s);
    }
    c
}

pub fn prove(circuit: &SelCircuit, t: Fr, n: Fr, ic: Fr, cc: Fr, m: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;
    let params_kzg = pruv_circuits::srs::get(K)?;
    let empty = SelCircuit::empty(m);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params_kzg, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![t, n, ic, cc]]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params_kzg, &pk, &[circuit.clone()], instances, OsRng, &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], t: Fr, n: Fr, ic: Fr, cc: Fr, m: usize) -> Result<bool> {
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
    let empty = SelCircuit::empty(m);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params_kzg.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![t, n, ic, cc]]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params, &vk, instances, &mut transcript,
    ))
}

pub fn run_selection() -> Result<()> {
    let wallets: Vec<Fr> = vec![Fr::from(111u64), Fr::from(222u64), Fr::from(333u64)];
    let scores: Vec<u64> = vec![120, 80, 200];
    let t: u64 = 100;
    let m = scores.len();
    let n = scores.iter().filter(|&&s| s >= t).count() as u64;
    let selected: Vec<Fr> = wallets
        .iter()
        .zip(scores.iter())
        .map(|(w, s)| if *s >= t { *w } else { Fr::from(0u64) })
        .collect();
    let ic = native_input_c(&wallets, &scores);
    let cc = native_claim_c(&selected);

    let circuit = SelCircuit {
        wallets: wallets.iter().map(|w| Value::known(*w)).collect(),
        scores: scores.iter().map(|&s| Value::known(s)).collect(),
        t: Value::known(t),
    };

    eprintln!("proving top-N winners (by identity) over M={m} committed candidates …");
    let proof = prove(&circuit, Fr::from(t), Fr::from(n), ic, cc, m)?;
    ensure!(verify(&proof, Fr::from(t), Fr::from(n), ic, cc, m)?, "verify failed");
    println!("✓ selection proof verified — winners are exactly the top-N, bound to identity");
    println!("  N={n} winners of M={m}; claimC commits the winner wallets, inputC the candidates");
    println!("  proof_bytes = {} B  (k={K})", proof.len());

    ensure!(!verify(&proof, Fr::from(t), Fr::from(n + 1), ic, cc, m)?, "wrong N verified");
    println!("  negative (N+1)            : rejected ✓");

    // tamper a winner's wallet but keep the committed claimC → must fail
    let mut bad_w = wallets.clone();
    bad_w[0] += Fr::from(1u64);
    let bad = SelCircuit {
        wallets: bad_w.iter().map(|w| Value::known(*w)).collect(),
        scores: scores.iter().map(|&s| Value::known(s)).collect(),
        t: Value::known(t),
    };
    let rejected = match prove(&bad, Fr::from(t), Fr::from(n), ic, cc, m) {
        Ok(pf) => !verify(&pf, Fr::from(t), Fr::from(n), ic, cc, m)?,
        Err(_) => true,
    };
    ensure!(rejected, "binding broken: tampered winner wallet matched the commitments");
    println!("  binding (swap winner wallet): rejected ✓");
    Ok(())
}

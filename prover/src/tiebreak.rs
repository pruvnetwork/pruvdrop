//! Tie-break: top-N under a total order (score desc, then index asc), so boundary ties
//! are resolved deterministically.
//!
//! A pure threshold can't pick exactly N when several candidates share the cutoff score.
//! Here a candidate wins iff `(score_i, i)` is lexicographically ≥ the public boundary
//! `(t_s, t_idx)`:  `win_i = [score_i > t_s]  OR  ([score_i == t_s] AND [i ≤ t_idx])`.
//! Among tied scores, lower index wins. `Σ win_i = N` (public) then pins the top-N even
//! through ties. Public (instance): `[t_s, t_idx, N]`.
//!
//! Run:  cargo run --release -- --tiebreak

use anyhow::{anyhow, ensure, Result};
use ff::Field;
use halo2_proofs::{
    circuit::{AssignedCell, Layouter, SimpleFloorPlanner, Value},
    halo2curves::bn256::{Fr, G1Affine},
    plonk::{
        Advice, Circuit, Column, ConstraintSystem, ErrorFront, Expression, Fixed, Instance,
        Selector,
    },
    poly::Rotation,
};

const BS: usize = 16; // score bit width
const BI: usize = 8; // index bit width (M < 256)
const K: u32 = 11;

#[derive(Clone)]
pub struct TbConfig {
    score: Column<Advice>,
    ts: Column<Advice>,
    tidx: Column<Advice>,
    abits: Vec<Column<Advice>>, // BS+1
    eq: Column<Advice>,
    inv: Column<Advice>,
    lebits: Vec<Column<Advice>>, // BI+1
    win: Column<Advice>,
    acc: Column<Advice>,
    idx: Column<Fixed>,
    constants: Column<Fixed>,
    q: Selector,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct TbCircuit {
    pub scores: Vec<Value<u64>>,
    pub ts: Value<u64>,
    pub tidx: Value<u64>,
}

impl TbCircuit {
    pub fn empty(m: usize) -> Self {
        Self { scores: vec![Value::unknown(); m], ts: Value::unknown(), tidx: Value::unknown() }
    }
}

impl Circuit<Fr> for TbCircuit {
    type Config = TbConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::empty(self.scores.len())
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> TbConfig {
        let score = meta.advice_column();
        let ts = meta.advice_column();
        let tidx = meta.advice_column();
        let abits: Vec<Column<Advice>> = (0..=BS).map(|_| meta.advice_column()).collect();
        let eq = meta.advice_column();
        let inv = meta.advice_column();
        let lebits: Vec<Column<Advice>> = (0..=BI).map(|_| meta.advice_column()).collect();
        let win = meta.advice_column();
        let acc = meta.advice_column();
        let idx = meta.fixed_column();
        let constants = meta.fixed_column();
        let q = meta.selector();
        let instance = meta.instance_column();

        for c in [ts, tidx, acc] {
            meta.enable_equality(c);
        }
        meta.enable_equality(instance);
        meta.enable_constant(constants);

        let abits_c = abits.clone();
        let lebits_c = lebits.clone();
        meta.create_gate("tiebreak", |meta| {
            let q = meta.query_selector(q);
            let score = meta.query_advice(score, Rotation::cur());
            let ts = meta.query_advice(ts, Rotation::cur());
            let tidx = meta.query_advice(tidx, Rotation::cur());
            let eq = meta.query_advice(eq, Rotation::cur());
            let inv = meta.query_advice(inv, Rotation::cur());
            let win = meta.query_advice(win, Rotation::cur());
            let acc_cur = meta.query_advice(acc, Rotation::cur());
            let acc_next = meta.query_advice(acc, Rotation::next());
            let idx = meta.query_fixed(idx, Rotation::cur());
            let one = Expression::Constant(Fr::from(1u64));
            let mut cons: Vec<Expression<Fr>> = Vec::new();

            // a = [score > t_s] = [score ≥ t_s + 1] via BS-bit decomposition of (score - t_s - 1 + 2^BS)
            let aq: Vec<Expression<Fr>> = abits_c.iter().map(|b| meta.query_advice(*b, Rotation::cur())).collect();
            for b in &aq {
                cons.push(q.clone() * b.clone() * (b.clone() - one.clone()));
            }
            let mut asum = Expression::Constant(Fr::from(0u64));
            for (k, b) in aq.iter().enumerate() {
                asum = asum + b.clone() * Expression::Constant(Fr::from(1u64 << k));
            }
            cons.push(q.clone() * (asum - (score.clone() - ts.clone() - one.clone() + Expression::Constant(Fr::from(1u64 << BS)))));
            let a = aq[BS].clone();

            // eq = [score == t_s] via is-zero of d = score - t_s
            let d = score - ts;
            cons.push(q.clone() * eq.clone() * d.clone()); // eq·d = 0
            cons.push(q.clone() * (eq.clone() + d * inv - one.clone())); // eq = 1 - d·inv

            // le = [i ≤ t_idx] = [t_idx - i ≥ 0] via BI-bit decomposition of (t_idx - idx + 2^BI)
            let lq: Vec<Expression<Fr>> = lebits_c.iter().map(|b| meta.query_advice(*b, Rotation::cur())).collect();
            for b in &lq {
                cons.push(q.clone() * b.clone() * (b.clone() - one.clone()));
            }
            let mut lsum = Expression::Constant(Fr::from(0u64));
            for (k, b) in lq.iter().enumerate() {
                lsum = lsum + b.clone() * Expression::Constant(Fr::from(1u64 << k));
            }
            cons.push(q.clone() * (lsum - (tidx - idx + Expression::Constant(Fr::from(1u64 << BI)))));
            let le = lq[BI].clone();

            // win = a + eq·le   (a and eq are mutually exclusive)
            cons.push(q.clone() * (win.clone() - (a + eq * le)));
            // running count
            cons.push(q.clone() * (acc_next - acc_cur - win));
            cons
        });

        TbConfig { score, ts, tidx, abits, eq, inv, lebits, win, acc, idx, constants, q, instance }
    }

    fn synthesize(&self, config: TbConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.scores.len();
        let (ts_cells, tidx_cells, acc_final) = layouter.assign_region(
            || "tiebreak",
            |mut region| {
                let mut acc_val: Value<u64> = Value::known(0);
                let acc0 = region.assign_advice(|| "acc0", config.acc, 0, || acc_val.map(Fr::from))?;
                region.constrain_constant(acc0.cell(), Fr::from(0u64))?;
                let mut ts_cells = Vec::with_capacity(m);
                let mut tidx_cells = Vec::with_capacity(m);
                let mut acc_cell = acc0;

                for i in 0..m {
                    config.q.enable(&mut region, i)?;
                    region.assign_fixed(|| "idx", config.idx, i, || Value::known(Fr::from(i as u64)))?;
                    let score = self.scores[i];
                    let ts = self.ts;
                    let tidx = self.tidx;

                    region.assign_advice(|| "score", config.score, i, || score.map(Fr::from))?;
                    ts_cells.push(region.assign_advice(|| "ts", config.ts, i, || ts.map(Fr::from))?);
                    tidx_cells.push(region.assign_advice(|| "tidx", config.tidx, i, || tidx.map(Fr::from))?);

                    // a-bits
                    let adiff: Value<u64> = score.zip(ts).map(|(s, t)| (s as i64 - t as i64 - 1 + (1i64 << BS)) as u64);
                    for k in 0..=BS {
                        region.assign_advice(|| "abit", config.abits[k], i, || adiff.map(|d| Fr::from((d >> k) & 1)))?;
                    }
                    // eq + inv (is-zero of score - t_s)
                    let dfr = score.zip(ts).map(|(s, t)| Fr::from(s) - Fr::from(t));
                    let eqv = score.zip(ts).map(|(s, t)| if s == t { Fr::from(1u64) } else { Fr::from(0u64) });
                    let invv = dfr.map(|d| Option::<Fr>::from(d.invert()).unwrap_or(Fr::from(0u64)));
                    region.assign_advice(|| "eq", config.eq, i, || eqv)?;
                    region.assign_advice(|| "inv", config.inv, i, || invv)?;
                    // le-bits
                    let ldiff: Value<u64> = tidx.map(|td| (td as i64 - i as i64 + (1i64 << BI)) as u64);
                    for k in 0..=BI {
                        region.assign_advice(|| "lebit", config.lebits[k], i, || ldiff.map(|d| Fr::from((d >> k) & 1)))?;
                    }
                    // win = a + eq·le
                    let a = adiff.map(|d| (d >> BS) & 1);
                    let le = ldiff.map(|d| (d >> BI) & 1);
                    let win_val: Value<u64> = a.zip(le).zip(score.zip(ts)).map(|((av, lev), (s, t))| {
                        if s > t { 1 } else if s == t && lev == 1 { 1 } else { 0 }
                    });
                    region.assign_advice(|| "win", config.win, i, || win_val.map(Fr::from))?;

                    acc_val = acc_val.zip(win_val).map(|(acc, w)| acc + w);
                    acc_cell = region.assign_advice(|| "acc", config.acc, i + 1, || acc_val.map(Fr::from))?;
                }
                Ok((ts_cells, tidx_cells, acc_cell))
            },
        )?;

        for c in &ts_cells {
            layouter.constrain_instance(c.cell(), config.instance, 0)?;
        }
        for c in &tidx_cells {
            layouter.constrain_instance(c.cell(), config.instance, 1)?;
        }
        layouter.constrain_instance(acc_final.cell(), config.instance, 2)?;
        Ok(())
    }
}

pub fn prove(circuit: &TbCircuit, ts: Fr, tidx: Fr, n: Fr, m: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;
    let params = pruv_circuits::srs::get(K)?;
    let empty = TbCircuit::empty(m);
    let vk = keygen_vk(&*params, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![ts, tidx, n]]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params, &pk, &[circuit.clone()], instances, OsRng, &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], ts: Fr, tidx: Fr, n: Fr, m: usize) -> Result<bool> {
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
    let params = pruv_circuits::srs::get(K)?;
    let empty = TbCircuit::empty(m);
    let vk = keygen_vk(&*params, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![ts, tidx, n]]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params, &vk, instances, &mut transcript,
    ))
}

pub fn run_tiebreak() -> Result<()> {
    // three candidates tie at the boundary score 30; top-3 must pick exactly 3
    let scores: Vec<u64> = vec![50, 30, 30, 30, 10];
    let m = scores.len();
    let ts: u64 = 30; // boundary score
    let tidx: u64 = 2; // last winner index among the tied group (i0,i1,i2 win; i3 loses)
    let n: u64 = scores
        .iter()
        .enumerate()
        .filter(|(i, &s)| s > ts || (s == ts && *i as u64 <= tidx))
        .count() as u64;

    let circuit = TbCircuit {
        scores: scores.iter().map(|&s| Value::known(s)).collect(),
        ts: Value::known(ts),
        tidx: Value::known(tidx),
    };

    eprintln!("proving top-N with tie-break: scores {scores:?}, boundary (t_s={ts}, t_idx={tidx}) …");
    let proof = prove(&circuit, Fr::from(ts), Fr::from(tidx), Fr::from(n), m)?;
    ensure!(verify(&proof, Fr::from(ts), Fr::from(tidx), Fr::from(n), m)?, "verify failed");
    println!("✓ tie-break proof verified — exactly N={n} chosen through a 3-way boundary tie");
    println!("  order = score desc, then index asc; winners = i0,i1,i2 (i3 loses the tie)");
    println!("  proof_bytes = {} B  (k={K})", proof.len());

    ensure!(!verify(&proof, Fr::from(ts), Fr::from(tidx), Fr::from(n + 1), m)?, "wrong N verified");
    println!("  negative (N+1)              : rejected ✓");
    // claiming t_idx = 3 would include i3 (a different N=4 selection); with N fixed it must fail
    ensure!(!verify(&proof, Fr::from(ts), Fr::from(3u64), Fr::from(n), m)?, "wrong boundary verified");
    println!("  negative (t_idx shifted)    : rejected ✓");
    Ok(())
}

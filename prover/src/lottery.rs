//! Weighted-lottery (anti-whale) selection, proven sound.
//!
//! Each candidate `j` owns a cumulative-weight bucket `[prefix[j], prefix[j]+score[j])`
//! whose **width is its score** — so selection probability ∝ score, and a small-score
//! candidate still has a nonzero chance (unlike top-N). For each public draw `r_k`, the
//! circuit proves it lands in exactly one bucket: `Σ_j [prefix[j] ≤ r_k < prefix[j]+score[j]] = 1`.
//! Prefix sums are constrained (`prefix[0]=0`, `prefix[M]=S`).
//!
//! The draws are public here; in production `r_k = Poseidon(seed, k) mod S` with `seed` a
//! commit-before-reveal slot hash (the sound bounded-mod reduction is the documented add-on).
//! Public (instance): `[S, r_0, …, r_{N-1}]`.  Run:  cargo run --release -- --lottery

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

const BS: usize = 16; // weight/prefix bit width (S < 2^16); comparator uses BS+1 bits
const N: usize = 2; // number of draws
const K: u32 = 12;

#[derive(Clone)]
pub struct LotConfig {
    weight: Column<Advice>,
    prefix: Column<Advice>,
    draws: Vec<Column<Advice>>,
    gebits: Vec<Vec<Column<Advice>>>,
    ltbits: Vec<Vec<Column<Advice>>>,
    ge: Vec<Column<Advice>>,
    lt: Vec<Column<Advice>>,
    inb: Vec<Column<Advice>>,
    acc: Vec<Column<Advice>>,
    constants: Column<Fixed>,
    q: Selector,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct LotCircuit {
    pub weights: Vec<Value<u64>>,
    pub draws: Vec<u64>,
}

impl LotCircuit {
    pub fn empty(m: usize) -> Self {
        Self { weights: vec![Value::unknown(); m], draws: vec![0; N] }
    }
}

fn recompose(bits: &[Expression<Fr>]) -> Expression<Fr> {
    let mut sum = Expression::Constant(Fr::from(0u64));
    for (k, b) in bits.iter().enumerate() {
        sum = sum + b.clone() * Expression::Constant(Fr::from(1u64 << k));
    }
    sum
}

impl Circuit<Fr> for LotCircuit {
    type Config = LotConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self { weights: vec![Value::unknown(); self.weights.len()], draws: self.draws.clone() }
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> LotConfig {
        let weight = meta.advice_column();
        let prefix = meta.advice_column();
        let draws: Vec<_> = (0..N).map(|_| meta.advice_column()).collect();
        let gebits: Vec<Vec<_>> = (0..N).map(|_| (0..=BS).map(|_| meta.advice_column()).collect()).collect();
        let ltbits: Vec<Vec<_>> = (0..N).map(|_| (0..=BS).map(|_| meta.advice_column()).collect()).collect();
        let ge: Vec<_> = (0..N).map(|_| meta.advice_column()).collect();
        let lt: Vec<_> = (0..N).map(|_| meta.advice_column()).collect();
        let inb: Vec<_> = (0..N).map(|_| meta.advice_column()).collect();
        let acc: Vec<_> = (0..N).map(|_| meta.advice_column()).collect();
        let constants = meta.fixed_column();
        let q = meta.selector();
        let instance = meta.instance_column();

        meta.enable_equality(prefix);
        for c in draws.iter().chain(acc.iter()) {
            meta.enable_equality(*c);
        }
        meta.enable_equality(instance);
        meta.enable_constant(constants);

        let (dr, geb, ltb, gec, ltc, inbc, accc) =
            (draws.clone(), gebits.clone(), ltbits.clone(), ge.clone(), lt.clone(), inb.clone(), acc.clone());
        meta.create_gate("lottery", |meta| {
            let q = meta.query_selector(q);
            let one = Expression::Constant(Fr::from(1u64));
            let off = Expression::Constant(Fr::from(1u64 << BS));
            let weight = meta.query_advice(weight, Rotation::cur());
            let p_cur = meta.query_advice(prefix, Rotation::cur());
            let p_next = meta.query_advice(prefix, Rotation::next());
            let mut cons: Vec<Expression<Fr>> = Vec::new();

            // prefix[j+1] = prefix[j] + weight[j]
            cons.push(q.clone() * (p_next.clone() - p_cur.clone() - weight));

            for k in 0..N {
                let r = meta.query_advice(dr[k], Rotation::cur());
                let geq: Vec<_> = geb[k].iter().map(|c| meta.query_advice(*c, Rotation::cur())).collect();
                let ltq: Vec<_> = ltb[k].iter().map(|c| meta.query_advice(*c, Rotation::cur())).collect();
                let ge = meta.query_advice(gec[k], Rotation::cur());
                let lt = meta.query_advice(ltc[k], Rotation::cur());
                let inb = meta.query_advice(inbc[k], Rotation::cur());
                let acc_c = meta.query_advice(accc[k], Rotation::cur());
                let acc_n = meta.query_advice(accc[k], Rotation::next());
                for b in geq.iter().chain(ltq.iter()) {
                    cons.push(q.clone() * b.clone() * (b.clone() - one.clone()));
                }
                // ge = [r ≥ prefix_cur]: recompose == r - prefix_cur + 2^BS ; ge = MSB
                cons.push(q.clone() * (recompose(&geq) - (r.clone() - p_cur.clone() + off.clone())));
                cons.push(q.clone() * (ge.clone() - geq[BS].clone()));
                // lt = [r < prefix_next]: recompose == prefix_next - 1 - r + 2^BS ; lt = MSB
                cons.push(q.clone() * (recompose(&ltq) - (p_next.clone() - one.clone() - r + off.clone())));
                cons.push(q.clone() * (lt.clone() - ltq[BS].clone()));
                // in_bucket = ge · lt ; running partition count
                cons.push(q.clone() * (inb.clone() - ge * lt));
                cons.push(q.clone() * (acc_n - acc_c - inb));
            }
            cons
        });

        LotConfig { weight, prefix, draws, gebits, ltbits, ge, lt, inb, acc, constants, q, instance }
    }

    fn synthesize(&self, config: LotConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.weights.len();
        let (prefix_final, acc_finals, draw_cells) = layouter.assign_region(
            || "lottery",
            |mut region| {
                let mut draw_cells: Vec<Vec<AssignedCell<Fr, Fr>>> = (0..N).map(|_| Vec::new()).collect();
                // prefix[0] = 0, acc_k[0] = 0
                let mut prefix_val: Value<u64> = Value::known(0);
                let p0 = region.assign_advice(|| "p0", config.prefix, 0, || prefix_val.map(Fr::from))?;
                region.constrain_constant(p0.cell(), Fr::from(0u64))?;
                let mut prefix_final_cell = p0.clone();
                let mut acc_vals = vec![Value::<u64>::known(0); N];
                for k in 0..N {
                    let a0 = region.assign_advice(|| "acc0", config.acc[k], 0, || acc_vals[k].map(Fr::from))?;
                    region.constrain_constant(a0.cell(), Fr::from(0u64))?;
                }

                for j in 0..m {
                    config.q.enable(&mut region, j)?;
                    let w = self.weights[j];
                    region.assign_advice(|| "weight", config.weight, j, || w.map(Fr::from))?;
                    let p_cur = prefix_val;
                    let p_next = prefix_val.zip(w).map(|(p, ww)| p + ww);
                    for k in 0..N {
                        let r = self.draws[k];
                        let dc = region.assign_advice(|| "draw", config.draws[k], j, || Value::known(Fr::from(r)))?;
                        draw_cells[k].push(dc);
                        let gediff = p_cur.map(|p| (r as i64 - p as i64 + (1i64 << BS)) as u64);
                        let ltdiff = p_next.map(|pn| (pn as i64 - 1 - r as i64 + (1i64 << BS)) as u64);
                        for b in 0..=BS {
                            region.assign_advice(|| "geb", config.gebits[k][b], j, || gediff.map(|d| Fr::from((d >> b) & 1)))?;
                            region.assign_advice(|| "ltb", config.ltbits[k][b], j, || ltdiff.map(|d| Fr::from((d >> b) & 1)))?;
                        }
                        let gev = gediff.map(|d| (d >> BS) & 1);
                        let ltv = ltdiff.map(|d| (d >> BS) & 1);
                        let inbv = gev.zip(ltv).map(|(g, l)| g * l);
                        region.assign_advice(|| "ge", config.ge[k], j, || gev.map(Fr::from))?;
                        region.assign_advice(|| "lt", config.lt[k], j, || ltv.map(Fr::from))?;
                        region.assign_advice(|| "inb", config.inb[k], j, || inbv.map(Fr::from))?;
                        acc_vals[k] = acc_vals[k].zip(inbv).map(|(a, b)| a + b);
                        region.assign_advice(|| "acc", config.acc[k], j + 1, || acc_vals[k].map(Fr::from))?;
                    }
                    prefix_final_cell = region.assign_advice(|| "prefix", config.prefix, j + 1, || p_next.map(Fr::from))?;
                    prefix_val = p_next;
                }
                let pf = prefix_final_cell;
                let mut acc_finals = Vec::with_capacity(N);
                for k in 0..N {
                    let af = region.assign_advice(|| "accf", config.acc[k], m, || acc_vals[k].map(Fr::from))?;
                    region.constrain_constant(af.cell(), Fr::from(1u64))?; // exactly one bucket per draw
                    acc_finals.push(af);
                }
                Ok((pf, acc_finals, draw_cells))
            },
        )?;

        layouter.constrain_instance(prefix_final.cell(), config.instance, 0)?; // S = Σ weight
        for k in 0..N {
            for c in &draw_cells[k] {
                layouter.constrain_instance(c.cell(), config.instance, 1 + k)?; // bind every row's draw to the public r_k
            }
        }
        let _ = acc_finals;
        Ok(())
    }
}

fn run() -> Result<(Vec<u64>, u64, Vec<u64>)> {
    let weights = vec![50u64, 30, 15, 5];
    let s: u64 = weights.iter().sum();
    let draws = vec![42u64, 88]; // 42 -> c0 [0,50); 88 -> c2 [80,95)
    Ok((weights, s, draws))
}

pub fn prove(circuit: &LotCircuit, s: Fr, draws: &[u64], m: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;
    let params = pruv_circuits::srs::get(K)?;
    let empty = LotCircuit::empty(m);
    let vk = keygen_vk(&*params, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;
    let mut pubs = vec![s];
    pubs.extend(draws.iter().map(|&d| Fr::from(d)));
    let instances: &[Vec<Vec<Fr>>] = &[vec![pubs]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params, &pk, &[circuit.clone()], instances, OsRng, &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], s: Fr, draws: &[u64], m: usize) -> Result<bool> {
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
    let empty = LotCircuit::empty(m);
    let vk = keygen_vk(&*params, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params.verifier_params().clone();
    let mut pubs = vec![s];
    pubs.extend(draws.iter().map(|&d| Fr::from(d)));
    let instances: &[Vec<Vec<Fr>>] = &[vec![pubs]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params, &vk, instances, &mut transcript,
    ))
}

pub fn run_lottery() -> Result<()> {
    let (weights, s, draws) = run()?;
    let m = weights.len();
    let circuit = LotCircuit {
        weights: weights.iter().map(|&w| Value::known(w)).collect(),
        draws: draws.clone(),
    };

    eprintln!("proving weighted lottery: draws {draws:?} over score-weighted buckets (S={s}) …");
    let proof = prove(&circuit, Fr::from(s), &draws, m)?;
    ensure!(verify(&proof, Fr::from(s), &draws, m)?, "verify failed");
    println!("✓ lottery proof verified — each draw lands in exactly one score-weighted bucket");
    println!("  weights {weights:?}  S={s}; draws {draws:?} → buckets c0,c2 (prob ∝ score)");
    println!("  proof_bytes = {} B  (k={K})", proof.len());

    // negative: claim a draw that doesn't fall where the prover says (shift S so buckets break)
    ensure!(!verify(&proof, Fr::from(s + 1), &draws, m)?, "wrong S verified");
    println!("  negative (wrong S)         : rejected ✓");
    Ok(())
}

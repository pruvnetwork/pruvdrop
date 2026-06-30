//! Phase-2 **spike**: the top-N counting/threshold core, as a real Halo2 circuit.
//!
//! Proves, over M witnessed scores, that **exactly N have score ≥ t**, where each
//! comparison `score_i ≥ t` is enforced by a bit-decomposition comparator and the
//! results are summed to N. `t` and `N` are public (instance column).
//!
//! This de-risks the novel mechanic of the Phase-2 allocation-correctness circuit
//! (the counting argument that replaces in-circuit sorting). It does NOT yet bind the
//! scores to a Poseidon `inputRoot` or compute payouts — those are added next.
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

const B: usize = 16; // score / t bit-width (scores fit in 16 bits)
const OFFSET: u64 = 1 << B; // 2^B, so diff = score - t + 2^B is non-negative
const TOPN_K: u32 = 11;

#[derive(Clone)]
pub struct TopNConfig {
    score: Column<Advice>,
    tcol: Column<Advice>,
    bits: Vec<Column<Advice>>, // B+1 bits
    win: Column<Advice>,
    acc: Column<Advice>,
    instance: Column<Instance>,
    constants: Column<Fixed>,
    q: Selector,
}

#[derive(Clone)]
pub struct TopNCircuit {
    pub scores: Vec<Value<u64>>,
    pub t: Value<u64>,
}

impl TopNCircuit {
    pub fn empty(m: usize) -> Self {
        Self { scores: vec![Value::unknown(); m], t: Value::unknown() }
    }
}

impl Circuit<Fr> for TopNCircuit {
    type Config = TopNConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::empty(self.scores.len())
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> TopNConfig {
        let score = meta.advice_column();
        let tcol = meta.advice_column();
        let bits: Vec<Column<Advice>> = (0..=B).map(|_| meta.advice_column()).collect();
        let win = meta.advice_column();
        let acc = meta.advice_column();
        let instance = meta.instance_column();
        let constants = meta.fixed_column();
        let q = meta.selector();

        meta.enable_equality(tcol);
        meta.enable_equality(acc);
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
            let bitq: Vec<Expression<Fr>> =
                bits_c.iter().map(|b| meta.query_advice(*b, Rotation::cur())).collect();

            let one = Expression::Constant(Fr::from(1u64));
            let mut cons: Vec<Expression<Fr>> = Vec::new();

            // booleanity of each bit
            for b in &bitq {
                cons.push(q.clone() * b.clone() * (b.clone() - one.clone()));
            }
            // win == most-significant bit (bit B)
            cons.push(q.clone() * (win.clone() - bitq[B].clone()));
            // recomposition: Σ bit_k 2^k == score - t + 2^B
            let mut sum = Expression::Constant(Fr::from(0u64));
            for (k, b) in bitq.iter().enumerate() {
                sum = sum + b.clone() * Expression::Constant(Fr::from(1u64 << k));
            }
            let target = score - tq + Expression::Constant(Fr::from(OFFSET));
            cons.push(q.clone() * (sum - target));
            // running count: acc_next == acc_cur + win
            cons.push(q.clone() * (acc_next - acc_cur - win));

            cons
        });

        TopNConfig { score, tcol, bits, win, acc, instance, constants, q }
    }

    fn synthesize(&self, config: TopNConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.scores.len();
        let (t_cells, acc_final) = layouter.assign_region(
            || "topn",
            |mut region| {
                // acc_0 = 0
                let mut acc_val: Value<u64> = Value::known(0);
                let acc0 = region.assign_advice(|| "acc0", config.acc, 0, || acc_val.map(Fr::from))?;
                region.constrain_constant(acc0.cell(), Fr::from(0u64))?;

                let mut t_cells: Vec<AssignedCell<Fr, Fr>> = Vec::with_capacity(m);
                let mut acc_cell = acc0;

                for i in 0..m {
                    config.q.enable(&mut region, i)?;
                    let score = self.scores[i];
                    let t = self.t;

                    region.assign_advice(|| "score", config.score, i, || score.map(Fr::from))?;
                    let tcell = region.assign_advice(|| "t", config.tcol, i, || t.map(Fr::from))?;
                    t_cells.push(tcell);

                    let diff: Value<u64> =
                        score.zip(t).map(|(s, tt)| (s as i64 - tt as i64 + OFFSET as i64) as u64);
                    for k in 0..=B {
                        region.assign_advice(
                            || "bit",
                            config.bits[k],
                            i,
                            || diff.map(|d| Fr::from((d >> k) & 1)),
                        )?;
                    }
                    let win_val: Value<u64> = diff.map(|d| (d >> B) & 1);
                    region.assign_advice(|| "win", config.win, i, || win_val.map(Fr::from))?;

                    acc_val = acc_val.zip(win_val).map(|(a, w)| a + w);
                    acc_cell = region.assign_advice(|| "acc", config.acc, i + 1, || acc_val.map(Fr::from))?;
                }
                Ok((t_cells, acc_cell))
            },
        )?;

        for tc in &t_cells {
            layouter.constrain_instance(tc.cell(), config.instance, 0)?; // public t
        }
        layouter.constrain_instance(acc_final.cell(), config.instance, 1)?; // public N
        Ok(())
    }
}

// ─── prove / verify (own keygen — not the fixed CircuitId cache) ────────────────

pub fn prove(circuit: &TopNCircuit, t: Fr, n: Fr, m: usize) -> Result<Vec<u8>> {
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

    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![t, n]]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params,
        &pk,
        &[circuit.clone()],
        instances,
        OsRng,
        &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], t: Fr, n: Fr, m: usize) -> Result<bool> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{keygen_pk, keygen_vk, verify_proof_multi},
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
    let _pk = keygen_pk(&*params, vk.clone(), &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;

    let verifier_params: ParamsVerifierKZG<Bn256> = params.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![t, n]]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    let ok = verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params,
        &vk,
        instances,
        &mut transcript,
    );
    Ok(ok)
}

pub fn run_spike() -> Result<()> {
    let scores: Vec<u64> = vec![120, 80, 200, 50, 300, 95, 40, 600];
    let t: u64 = 100;
    let m = scores.len();
    let n = scores.iter().filter(|&&s| s >= t).count() as u64;

    let circuit = TopNCircuit {
        scores: scores.iter().map(|&s| Value::known(s)).collect(),
        t: Value::known(t),
    };

    eprintln!("spike: proving Σ(score ≥ {t}) = {n} over M={m} …");
    let proof = prove(&circuit, Fr::from(t), Fr::from(n), m)?;
    let ok = verify(&proof, Fr::from(t), Fr::from(n), m)?;
    anyhow::ensure!(ok, "verify failed");
    println!("✓ spike proof verified");
    println!("  statement   : exactly N={n} of M={m} scores are ≥ t={t}");
    println!("  proof_bytes : {} B  (k={TOPN_K})", proof.len());

    // negative control: a false count must NOT verify
    let bad = verify(&proof, Fr::from(t), Fr::from(n + 1), m)?;
    anyhow::ensure!(!bad, "soundness bug: wrong N verified");
    println!("  negative test: claiming N+1 correctly rejected ✓");
    Ok(())
}

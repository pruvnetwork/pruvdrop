//! Proportional payout, proven sound: each winner's amount IS the correct floor of its
//! pot share, not a trusted witness.
//!
//! For N winners with scores `score_i` and `S = Σ score_i`, proves for every winner:
//!   `pot · score_i == amount_i · S + rem_i`,  with `0 ≤ rem_i < S`  and  `amount_i ≥ 0`,
//! i.e. `amount_i = ⌊pot · score_i / S⌋` (exact integer division, no field wraparound — all
//! values are range-bounded). Conservation: `Σ amount_i + dust == pot` with `0 ≤ dust < 2^Bd`
//! (flooring leftover, < N units). `S` is computed in-circuit (`Σ score_i`) and public.
//! Public (instance): `[pot, S]`. A tampered amount can't produce a valid `rem`.
//!
//! Scores are assumed range-bounded here (they are, via the comparator, in the full
//! allocation circuit). Run:  cargo run --release -- --proportional

use anyhow::{anyhow, ensure, Result};
use halo2_proofs::{
    circuit::{AssignedCell, Layouter, SimpleFloorPlanner, Value},
    halo2curves::bn256::{Fr, G1Affine},
    plonk::{
        Advice, Circuit, Column, ConstraintSystem, ErrorFront, Expression, Instance, Selector,
    },
    poly::Rotation,
};

const BW: usize = 20; // amount / rem / ltdiff bit width (< 2^20)
const BD: usize = 12; // dust bit width
const K: u32 = 14;

#[derive(Clone)]
pub struct PropConfig {
    score: Column<Advice>,
    amount: Column<Advice>,
    rem: Column<Advice>,
    abits: Vec<Column<Advice>>,
    rbits: Vec<Column<Advice>>,
    ltbits: Vec<Column<Advice>>,
    dbits: Vec<Column<Advice>>,
    scol: Column<Advice>,   // S, equal across rows
    pcol: Column<Advice>,   // pot, equal across rows
    accs: Column<Advice>,   // Σ score
    acca: Column<Advice>,   // Σ amount
    q_div: Selector,
    q_dust: Selector,
    constants: Column<halo2_proofs::plonk::Fixed>,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct PropCircuit {
    pub scores: Vec<Value<u64>>,
    pub amounts: Vec<Value<u64>>,
    pub rems: Vec<Value<u64>>,
    pub pot: Value<u64>,
    pub s: Value<u64>,
    pub dust: Value<u64>,
}

impl PropCircuit {
    pub fn empty(m: usize) -> Self {
        Self {
            scores: vec![Value::unknown(); m],
            amounts: vec![Value::unknown(); m],
            rems: vec![Value::unknown(); m],
            pot: Value::unknown(),
            s: Value::unknown(),
            dust: Value::unknown(),
        }
    }
}

fn recompose(bits: &[Expression<Fr>]) -> Expression<Fr> {
    let mut sum = Expression::Constant(Fr::from(0u64));
    for (k, b) in bits.iter().enumerate() {
        sum = sum + b.clone() * Expression::Constant(Fr::from(1u64 << k));
    }
    sum
}

impl Circuit<Fr> for PropCircuit {
    type Config = PropConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::empty(self.scores.len())
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> PropConfig {
        let score = meta.advice_column();
        let amount = meta.advice_column();
        let rem = meta.advice_column();
        let abits: Vec<_> = (0..BW).map(|_| meta.advice_column()).collect();
        let rbits: Vec<_> = (0..BW).map(|_| meta.advice_column()).collect();
        let ltbits: Vec<_> = (0..BW).map(|_| meta.advice_column()).collect();
        let dbits: Vec<_> = (0..BD).map(|_| meta.advice_column()).collect();
        let scol = meta.advice_column();
        let pcol = meta.advice_column();
        let accs = meta.advice_column();
        let acca = meta.advice_column();
        let q_div = meta.selector();
        let q_dust = meta.selector();
        let constants = meta.fixed_column();
        let instance = meta.instance_column();

        for c in [scol, pcol, accs, acca] {
            meta.enable_equality(c);
        }
        meta.enable_equality(instance);
        meta.enable_constant(constants);

        let (ab, rb, lb, db) = (abits.clone(), rbits.clone(), ltbits.clone(), dbits.clone());
        meta.create_gate("div", |meta| {
            let q = meta.query_selector(q_div);
            let one = Expression::Constant(Fr::from(1u64));
            let score = meta.query_advice(score, Rotation::cur());
            let amount = meta.query_advice(amount, Rotation::cur());
            let rem = meta.query_advice(rem, Rotation::cur());
            let scol_c = meta.query_advice(scol, Rotation::cur());
            let scol_n = meta.query_advice(scol, Rotation::next());
            let pcol = meta.query_advice(pcol, Rotation::cur());
            let accs_c = meta.query_advice(accs, Rotation::cur());
            let accs_n = meta.query_advice(accs, Rotation::next());
            let acca_c = meta.query_advice(acca, Rotation::cur());
            let acca_n = meta.query_advice(acca, Rotation::next());
            let aq: Vec<_> = ab.iter().map(|c| meta.query_advice(*c, Rotation::cur())).collect();
            let rq: Vec<_> = rb.iter().map(|c| meta.query_advice(*c, Rotation::cur())).collect();
            let lq: Vec<_> = lb.iter().map(|c| meta.query_advice(*c, Rotation::cur())).collect();

            let mut cons = Vec::new();
            for b in aq.iter().chain(rq.iter()).chain(lq.iter()) {
                cons.push(q.clone() * b.clone() * (b.clone() - one.clone()));
            }
            cons.push(q.clone() * (amount.clone() - recompose(&aq))); // amount ≥ 0, < 2^BW
            cons.push(q.clone() * (rem.clone() - recompose(&rq))); // rem ≥ 0
            cons.push(q.clone() * (recompose(&lq) - (scol_c.clone() - rem.clone() - one.clone()))); // S - rem - 1 ≥ 0 → rem < S
            cons.push(q.clone() * (pcol * score.clone() - (amount.clone() * scol_c.clone() + rem))); // pot·score = amount·S + rem
            cons.push(q.clone() * (accs_n - accs_c - score)); // Σ score
            cons.push(q.clone() * (acca_n - acca_c - amount)); // Σ amount
            cons.push(q.clone() * (scol_n - scol_c)); // S equal across rows
            cons
        });

        meta.create_gate("dust", |meta| {
            let q = meta.query_selector(q_dust);
            let pcol = meta.query_advice(pcol, Rotation::cur());
            let acca = meta.query_advice(acca, Rotation::cur());
            let dq: Vec<_> = db.iter().map(|c| meta.query_advice(*c, Rotation::cur())).collect();
            let one = Expression::Constant(Fr::from(1u64));
            let mut cons = Vec::new();
            for b in &dq {
                cons.push(q.clone() * b.clone() * (b.clone() - one.clone()));
            }
            // dust = pot - Σamount, 0 ≤ dust < 2^BD  → conservation (acca ≤ pot, leftover tiny)
            cons.push(q.clone() * (recompose(&dq) - (pcol - acca)));
            cons
        });

        PropConfig {
            score, amount, rem, abits, rbits, ltbits, dbits, scol, pcol, accs, acca,
            q_div, q_dust, constants, instance,
        }
    }

    fn synthesize(&self, config: PropConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.scores.len();
        let (scol_cells, pcol_cells, accs_final) = layouter.assign_region(
            || "proportional",
            |mut region| {
                let mut accs_val: Value<u64> = Value::known(0);
                let mut acca_val: Value<u64> = Value::known(0);
                let accs0 = region.assign_advice(|| "accs0", config.accs, 0, || accs_val.map(Fr::from))?;
                region.constrain_constant(accs0.cell(), Fr::from(0u64))?;
                let acca0 = region.assign_advice(|| "acca0", config.acca, 0, || acca_val.map(Fr::from))?;
                region.constrain_constant(acca0.cell(), Fr::from(0u64))?;

                let mut scol_cells = Vec::new();
                let mut pcol_cells = Vec::new();
                for i in 0..m {
                    config.q_div.enable(&mut region, i)?;
                    let score = self.scores[i];
                    let amount = self.amounts[i];
                    let rem = self.rems[i];
                    region.assign_advice(|| "score", config.score, i, || score.map(Fr::from))?;
                    region.assign_advice(|| "amount", config.amount, i, || amount.map(Fr::from))?;
                    region.assign_advice(|| "rem", config.rem, i, || rem.map(Fr::from))?;
                    scol_cells.push(region.assign_advice(|| "scol", config.scol, i, || self.s.map(Fr::from))?);
                    pcol_cells.push(region.assign_advice(|| "pcol", config.pcol, i, || self.pot.map(Fr::from))?);

                    for k in 0..BW {
                        region.assign_advice(|| "ab", config.abits[k], i, || amount.map(|v| Fr::from((v >> k) & 1)))?;
                        region.assign_advice(|| "rb", config.rbits[k], i, || rem.map(|v| Fr::from((v >> k) & 1)))?;
                        let lt = self.s.zip(rem).map(|(s, r)| s - r - 1);
                        region.assign_advice(|| "lb", config.ltbits[k], i, || lt.map(|v| Fr::from((v >> k) & 1)))?;
                    }
                    accs_val = accs_val.zip(score).map(|(a, s)| a + s);
                    acca_val = acca_val.zip(amount).map(|(a, am)| a + am);
                    region.assign_advice(|| "accs", config.accs, i + 1, || accs_val.map(Fr::from))?;
                    region.assign_advice(|| "acca", config.acca, i + 1, || acca_val.map(Fr::from))?;
                }
                // row m: scol (for the equality gate at row m-1) + dust
                let scol_m = region.assign_advice(|| "scol_m", config.scol, m, || self.s.map(Fr::from))?;
                let pcol_m = region.assign_advice(|| "pcol_m", config.pcol, m, || self.pot.map(Fr::from))?;
                let accs_final = region.assign_advice(|| "accs_f", config.accs, m, || accs_val.map(Fr::from))?;
                let _ = region.assign_advice(|| "acca_f", config.acca, m, || acca_val.map(Fr::from))?;
                // accs_final must equal S
                region.constrain_equal(accs_final.cell(), scol_m.cell())?;
                // dust gate at row m
                config.q_dust.enable(&mut region, m)?;
                for k in 0..BD {
                    region.assign_advice(|| "db", config.dbits[k], m, || self.dust.map(|v| Fr::from((v >> k) & 1)))?;
                }
                scol_cells.push(scol_m);
                pcol_cells.push(pcol_m);
                Ok((scol_cells, pcol_cells, accs_final))
            },
        )?;

        for pc in &pcol_cells {
            layouter.constrain_instance(pc.cell(), config.instance, 0)?; // pot
        }
        for sc in &scol_cells {
            layouter.constrain_instance(sc.cell(), config.instance, 1)?; // S
        }
        let _ = accs_final;
        Ok(())
    }
}

pub fn prove(circuit: &PropCircuit, pot: Fr, s: Fr, m: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;
    let params = pruv_circuits::srs::get(K)?;
    let empty = PropCircuit::empty(m);
    let vk = keygen_vk(&*params, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![pot, s]]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params, &pk, &[circuit.clone()], instances, OsRng, &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], pot: Fr, s: Fr, m: usize) -> Result<bool> {
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
    let empty = PropCircuit::empty(m);
    let vk = keygen_vk(&*params, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![pot, s]]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params, &vk, instances, &mut transcript,
    ))
}

pub fn run_proportional() -> Result<()> {
    let scores: Vec<u64> = vec![120, 80, 200, 50];
    let pot: u64 = 10_000;
    let m = scores.len();
    let s: u64 = scores.iter().sum();
    let amounts: Vec<u64> = scores.iter().map(|&sc| pot * sc / s).collect();
    let rems: Vec<u64> = scores.iter().zip(&amounts).map(|(&sc, &a)| pot * sc - a * s).collect();
    let dust = pot - amounts.iter().sum::<u64>();

    let circuit = PropCircuit {
        scores: scores.iter().map(|&v| Value::known(v)).collect(),
        amounts: amounts.iter().map(|&v| Value::known(v)).collect(),
        rems: rems.iter().map(|&v| Value::known(v)).collect(),
        pot: Value::known(pot),
        s: Value::known(s),
        dust: Value::known(dust),
    };

    eprintln!("proving proportional payout: amounts = ⌊pot·score/S⌋, pot={pot}, S={s} …");
    let proof = prove(&circuit, Fr::from(pot), Fr::from(s), m)?;
    ensure!(verify(&proof, Fr::from(pot), Fr::from(s), m)?, "verify failed");
    println!("✓ proportional proof verified — each amount IS ⌊pot·score/S⌋ (not trusted)");
    println!("  amounts = {amounts:?}  rems = {rems:?}  dust = {dust}  (Σamount+dust = {pot})");
    println!("  proof_bytes = {} B  (k={K})", proof.len());

    // negative: a tampered (non-floor) amount cannot produce a valid rem in [0, S)
    let mut bad = circuit.clone();
    bad.amounts[0] = Value::known(amounts[0] + 1);
    bad.rems[0] = Value::known(rems[0]); // keep old rem → relation breaks
    let rejected = match prove(&bad, Fr::from(pot), Fr::from(s), m) {
        Ok(pf) => !verify(&pf, Fr::from(pot), Fr::from(s), m)?,
        Err(_) => true,
    };
    ensure!(rejected, "soundness: a wrong (non-floor) amount verified");
    println!("  negative (amount not the floor): rejected ✓");
    Ok(())
}

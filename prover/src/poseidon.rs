//! Sound Poseidon-128 (BN254, width 3) — foundation for the in-circuit gadget.
//!
//! **Step 1 (this file):** extract light-poseidon's exact round constants (ARK) and MDS
//! matrix for width 3, and implement the permutation natively in halo2's `Fr`, proven
//! equal to `pruv_circuits::poseidon_hasher::hash_two`. This locks the constants +
//! algorithm so the in-circuit constrained permutation (Step 2) can be built and tested
//! against a trusted reference. (Fixes the gap where PRUV's `PoseidonChip` enforces no
//! hash gate — see docs/ZK-ALLOCATION-PROOF.md §7.)
//!
//! Algorithm (matches light-poseidon `new_circom(2)`, domain_tag = 0):
//!   state = [0, a, b]; for each round: ARK, S-box (x^5), MDS; result = state[0].
//!   S-box is full (all 3 lanes) on the first R_F/2 and last R_F/2 rounds, partial
//!   (lane 0 only) on the middle R_P rounds.
//!
//! Run:  cargo run --release -- --poseidon-test

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

const POSEIDON_K: u32 = 11;

fn from_ark(f: ark_bn254::Fr) -> Fr {
    use ark_ff::{BigInteger, PrimeField as ArkPF};
    use ff::PrimeField;
    let le = ArkPF::into_bigint(f).to_bytes_le();
    let mut buf = [0u8; 32];
    buf[..le.len().min(32)].copy_from_slice(&le[..le.len().min(32)]);
    Fr::from_repr(buf.into()).expect("ark→halo2 conversion")
}

pub struct Params {
    pub ark: Vec<Fr>,      // flattened, indexed [round * width + lane]
    pub mds: Vec<Vec<Fr>>, // width × width
    pub full_rounds: usize,
    pub partial_rounds: usize,
    pub width: usize,
    pub alpha: u64,
}

/// Exact light-poseidon width-3 (2-input) constants, converted to halo2 `Fr`.
pub fn params() -> Params {
    let p = light_poseidon::parameters::bn254_x5::get_poseidon_parameters::<ark_bn254::Fr>(3)
        .expect("bn254 x5 width-3 params");
    Params {
        ark: p.ark.iter().map(|x| from_ark(*x)).collect(),
        mds: p.mds.iter().map(|row| row.iter().map(|x| from_ark(*x)).collect()).collect(),
        full_rounds: p.full_rounds,
        partial_rounds: p.partial_rounds,
        width: p.width,
        alpha: p.alpha,
    }
}

#[inline]
fn pow5(x: Fr) -> Fr {
    let x2 = x * x;
    let x4 = x2 * x2;
    x4 * x
}

/// Native Poseidon permutation for 2 inputs, in halo2 `Fr`.
pub fn perm_native(a: Fr, b: Fr) -> Fr {
    let p = params();
    let w = p.width;
    let half = p.full_rounds / 2;
    let all = p.full_rounds + p.partial_rounds;
    let zero = Fr::from(0u64);

    let mut state = vec![zero, a, b];
    for round in 0..all {
        // ARK
        for i in 0..w {
            state[i] += p.ark[round * w + i];
        }
        // S-box
        let full = round < half || round >= half + p.partial_rounds;
        if full {
            for s in state.iter_mut() {
                *s = pow5(*s);
            }
        } else {
            state[0] = pow5(state[0]);
        }
        // MDS
        let mut next = vec![zero; w];
        for i in 0..w {
            let mut acc = zero;
            for j in 0..w {
                acc += state[j] * p.mds[i][j];
            }
            next[i] = acc;
        }
        state = next;
    }
    state[0]
}

/// Verify the native permutation matches the trusted reference (pruv hash_two).
pub fn run_test() -> Result<()> {
    let cases = [
        (Fr::from(1u64), Fr::from(2u64)),
        (Fr::from(0u64), Fr::from(0u64)),
        (Fr::from(123_456_789u64), Fr::from(987_654_321u64)),
        (Fr::from(u64::MAX), Fr::from(7u64)),
        (Fr::from(42u64), Fr::from(u64::MAX)),
    ];
    for (a, b) in cases {
        let mine = perm_native(a, b);
        let reference = pruv_circuits::poseidon_hasher::hash_two(a, b);
        ensure!(mine == reference, "perm mismatch vs hash_two for ({a:?},{b:?})");
    }
    let p = params();
    println!("✓ native Poseidon perm matches pruv hash_two on {} cases", cases.len());
    println!(
        "  width={} full_rounds={} partial_rounds={} alpha={} ark_len={} mds={}x{}",
        p.width, p.full_rounds, p.partial_rounds, p.alpha, p.ark.len(), p.mds.len(), p.mds[0].len()
    );
    println!("  → constants + algorithm locked; Step 2 builds the in-circuit constrained permutation");
    Ok(())
}

// ─── Step 2: in-circuit constrained permutation ─────────────────────────────────

/// State at every row: trace[0] = [0,a,b]; trace[r+1] = state after round r.
pub fn perm_trace(a: Fr, b: Fr) -> Vec<[Fr; 3]> {
    let p = params();
    let half = p.full_rounds / 2;
    let all = p.full_rounds + p.partial_rounds;
    let zero = Fr::from(0u64);
    let mut state = [zero, a, b];
    let mut trace = vec![state];
    for round in 0..all {
        let mut s = state;
        for i in 0..3 {
            s[i] += p.ark[round * 3 + i];
        }
        let full = round < half || round >= half + p.partial_rounds;
        if full {
            for v in s.iter_mut() {
                *v = pow5(*v);
            }
        } else {
            s[0] = pow5(s[0]);
        }
        let mut n = [zero; 3];
        for i in 0..3 {
            let mut acc = zero;
            for j in 0..3 {
                acc += s[j] * p.mds[i][j];
            }
            n[i] = acc;
        }
        state = n;
        trace.push(state);
    }
    trace
}

#[derive(Clone)]
pub struct PoseidonConfig {
    s: [Column<Advice>; 3],
    ark: [Column<Fixed>; 3],
    mds: [[Column<Fixed>; 3]; 3],
    constant: Column<Fixed>,
    q_full: Selector,
    q_part: Selector,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct PoseidonHashCircuit {
    pub a: Value<Fr>,
    pub b: Value<Fr>,
}

impl Circuit<Fr> for PoseidonHashCircuit {
    type Config = PoseidonConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self { a: Value::unknown(), b: Value::unknown() }
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> PoseidonConfig {
        let s = [meta.advice_column(), meta.advice_column(), meta.advice_column()];
        for c in s {
            meta.enable_equality(c);
        }
        let ark = [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()];
        let mds = [
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
        ];
        let constant = meta.fixed_column();
        meta.enable_constant(constant);
        let instance = meta.instance_column();
        meta.enable_equality(instance);
        let q_full = meta.selector();
        let q_part = meta.selector();

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
            // S-box on lane 0 only; lanes 1,2 carry their post-ARK value
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

        PoseidonConfig { s, ark, mds, constant, q_full, q_part, instance }
    }

    fn synthesize(&self, config: PoseidonConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let p = params();
        let half = p.full_rounds / 2;
        let all = p.full_rounds + p.partial_rounds;
        let trace = self.a.zip(self.b).map(|(a, b)| perm_trace(a, b));

        let (a_cell, b_cell, out_cell) = layouter.assign_region(
            || "poseidon_perm",
            |mut region| {
                for r in 0..all {
                    for i in 0..3 {
                        region.assign_fixed(|| "ark", config.ark[i], r, || Value::known(p.ark[r * 3 + i]))?;
                    }
                    for i in 0..3 {
                        for j in 0..3 {
                            region.assign_fixed(|| "mds", config.mds[i][j], r, || Value::known(p.mds[i][j]))?;
                        }
                    }
                    let full = r < half || r >= half + p.partial_rounds;
                    if full {
                        config.q_full.enable(&mut region, r)?;
                    } else {
                        config.q_part.enable(&mut region, r)?;
                    }
                }

                let mut rows: Vec<Vec<AssignedCell<Fr, Fr>>> = Vec::with_capacity(all + 1);
                for r in 0..=all {
                    let mut row = Vec::with_capacity(3);
                    for i in 0..3 {
                        let v = trace.clone().map(|t| t[r][i]);
                        row.push(region.assign_advice(|| "s", config.s[i], r, || v)?);
                    }
                    rows.push(row);
                }
                region.constrain_constant(rows[0][0].cell(), Fr::from(0u64))?;
                Ok((rows[0][1].clone(), rows[0][2].clone(), rows[all][0].clone()))
            },
        )?;

        layouter.constrain_instance(a_cell.cell(), config.instance, 0)?;
        layouter.constrain_instance(b_cell.cell(), config.instance, 1)?;
        layouter.constrain_instance(out_cell.cell(), config.instance, 2)?;
        Ok(())
    }
}

pub fn prove_hash(a: Fr, b: Fr) -> Result<(Vec<u8>, Fr)> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;

    let out = perm_native(a, b);
    let params_kzg = pruv_circuits::srs::get(POSEIDON_K)?;
    let empty = PoseidonHashCircuit { a: Value::unknown(), b: Value::unknown() };
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params_kzg, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;

    let circuit = PoseidonHashCircuit { a: Value::known(a), b: Value::known(b) };
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![a, b, out]]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params_kzg,
        &pk,
        &[circuit],
        instances,
        OsRng,
        &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok((transcript.finalize(), out))
}

pub fn verify_hash(proof: &[u8], a: Fr, b: Fr, out: Fr) -> Result<bool> {
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

    let params_kzg = pruv_circuits::srs::get(POSEIDON_K)?;
    let empty = PoseidonHashCircuit { a: Value::unknown(), b: Value::unknown() };
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params_kzg.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![a, b, out]]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params,
        &vk,
        instances,
        &mut transcript,
    ))
}

/// Prove + verify a fully-constrained in-circuit Poseidon(a,b)=out.
pub fn run_gadget_test() -> Result<()> {
    let a = Fr::from(123u64);
    let b = Fr::from(456u64);

    eprintln!("proving in-circuit Poseidon(123,456) = hash_two(123,456) …");
    let (proof, out) = prove_hash(a, b)?;
    ensure!(out == pruv_circuits::poseidon_hasher::hash_two(a, b), "native out != hash_two");
    ensure!(verify_hash(&proof, a, b, out)?, "verify failed");
    println!("✓ in-circuit Poseidon proof verified (every ARK/S-box/MDS constrained)");
    println!("  Poseidon(123,456) bound to public output");
    println!("  proof_bytes = {} B  (k={POSEIDON_K})", proof.len());

    ensure!(!verify_hash(&proof, a, b, out + Fr::from(1u64))?, "wrong output verified");
    println!("  negative (wrong output) rejected ✓");
    Ok(())
}

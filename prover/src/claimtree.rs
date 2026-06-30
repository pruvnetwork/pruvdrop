//! Sound in-circuit Poseidon-**Merkle** claim root (vs the linear chain).
//!
//! Builds a Poseidon Merkle tree over the M claim leaves bottom-up — at each level
//! `parent = Poseidon(left, right)` via the constrained permutation, children linked by
//! copy constraints — and constrains the top to the public `claimRoot`. A Merkle root
//! (not a chain) lets the on-chain claim program verify each recipient with an O(log M)
//! path instead of replaying the whole commitment.
//!
//! Leaves here are direct values; in the full circuit a leaf is `Poseidon(wallet, amount)`
//! (or with index). Public (instance): `[claimRoot]`.  Run:  cargo run --release -- --claimtree

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

const K: u32 = 12;

#[derive(Clone)]
pub struct CtConfig {
    leaf: Column<Advice>,
    s: [Column<Advice>; 3],
    ark: [Column<Fixed>; 3],
    mds: [[Column<Fixed>; 3]; 3],
    constants: Column<Fixed>,
    q_full: Selector,
    q_part: Selector,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct CtCircuit {
    pub leaves: Vec<Value<Fr>>,
}

impl CtCircuit {
    pub fn empty(m: usize) -> Self {
        Self { leaves: vec![Value::unknown(); m] }
    }
}

fn perm_gates(
    meta: &mut ConstraintSystem<Fr>,
    s: [Column<Advice>; 3],
    ark: [Column<Fixed>; 3],
    mds: [[Column<Fixed>; 3]; 3],
    q_full: Selector,
    q_part: Selector,
) {
    let pow5e = |e: Expression<Fr>| {
        let e2 = e.clone() * e.clone();
        let e4 = e2.clone() * e2.clone();
        e4 * e
    };
    let mk = |meta: &mut ConstraintSystem<Fr>, sel: Selector, partial: bool| {
        meta.create_gate(if partial { "partial" } else { "full" }, move |meta| {
            let q = meta.query_selector(sel);
            let s_cur: Vec<_> = (0..3).map(|i| meta.query_advice(s[i], Rotation::cur())).collect();
            let s_next: Vec<_> = (0..3).map(|i| meta.query_advice(s[i], Rotation::next())).collect();
            let ark_q: Vec<_> = (0..3).map(|i| meta.query_fixed(ark[i], Rotation::cur())).collect();
            let mds_q: Vec<Vec<_>> = (0..3)
                .map(|i| (0..3).map(|j| meta.query_fixed(mds[i][j], Rotation::cur())).collect())
                .collect();
            let sb: Vec<Expression<Fr>> = if partial {
                vec![
                    pow5e(s_cur[0].clone() + ark_q[0].clone()),
                    s_cur[1].clone() + ark_q[1].clone(),
                    s_cur[2].clone() + ark_q[2].clone(),
                ]
            } else {
                (0..3).map(|j| pow5e(s_cur[j].clone() + ark_q[j].clone())).collect()
            };
            (0..3)
                .map(|i| {
                    let rhs = (0..3).fold(Expression::Constant(Fr::from(0u64)), |acc, j| {
                        acc + sb[j].clone() * mds_q[i][j].clone()
                    });
                    q.clone() * (s_next[i].clone() - rhs)
                })
                .collect::<Vec<_>>()
        });
    };
    mk(meta, q_full, false);
    mk(meta, q_part, true);
}

/// One sound Poseidon node: parent = Poseidon(left, right), children copied in.
fn node(
    layouter: &mut impl Layouter<Fr>,
    cfg: &CtConfig,
    left: &AssignedCell<Fr, Fr>,
    right: &AssignedCell<Fr, Fr>,
) -> Result<AssignedCell<Fr, Fr>, ErrorFront> {
    let p = params();
    let half = p.full_rounds / 2;
    let all = p.full_rounds + p.partial_rounds;
    let trace = left.value().copied().zip(right.value().copied()).map(|(l, r)| perm_trace(l, r));
    layouter.assign_region(
        || "node",
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
                if full { cfg.q_full.enable(&mut region, r)?; } else { cfg.q_part.enable(&mut region, r)?; }
            }
            let s0 = region.assign_advice(|| "s0", cfg.s[0], 0, || Value::known(Fr::from(0u64)))?;
            region.constrain_constant(s0.cell(), Fr::from(0u64))?;
            left.copy_advice(|| "L", &mut region, cfg.s[1], 0)?;
            right.copy_advice(|| "R", &mut region, cfg.s[2], 0)?;
            let mut parent = None;
            for r in 1..=all {
                for k in 0..3 {
                    let v = trace.clone().map(|t| t[r][k]);
                    let cell = region.assign_advice(|| "s", cfg.s[k], r, || v)?;
                    if r == all && k == 0 { parent = Some(cell); }
                }
            }
            Ok(parent.unwrap())
        },
    )
}

impl Circuit<Fr> for CtCircuit {
    type Config = CtConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::empty(self.leaves.len())
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> CtConfig {
        let leaf = meta.advice_column();
        let s = [meta.advice_column(), meta.advice_column(), meta.advice_column()];
        let ark = [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()];
        let mds = [
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
        ];
        let constants = meta.fixed_column();
        let q_full = meta.selector();
        let q_part = meta.selector();
        let instance = meta.instance_column();

        for c in [leaf, s[0], s[1], s[2]] {
            meta.enable_equality(c);
        }
        meta.enable_equality(instance);
        meta.enable_constant(constants);
        perm_gates(meta, s, ark, mds, q_full, q_part);

        CtConfig { leaf, s, ark, mds, constants, q_full, q_part, instance }
    }

    fn synthesize(&self, config: CtConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let m = self.leaves.len();
        // assign leaves as cells
        let mut level: Vec<AssignedCell<Fr, Fr>> = layouter.assign_region(
            || "leaves",
            |mut region| {
                let mut cells = Vec::with_capacity(m);
                for i in 0..m {
                    cells.push(region.assign_advice(|| "leaf", config.leaf, i, || self.leaves[i])?);
                }
                Ok(cells)
            },
        )?;

        // build the tree bottom-up
        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len() / 2);
            let mut i = 0;
            while i < level.len() {
                next.push(node(&mut layouter, &config, &level[i], &level[i + 1])?);
                i += 2;
            }
            level = next;
        }

        layouter.constrain_instance(level[0].cell(), config.instance, 0)?; // claimRoot
        Ok(())
    }
}

fn full_tree_root(leaves: &[Fr]) -> Fr {
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        level = level.chunks(2).map(|p| perm_native(p[0], p[1])).collect();
    }
    level[0]
}

pub fn prove(circuit: &CtCircuit, root: Fr, m: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;
    let params_kzg = pruv_circuits::srs::get(K)?;
    let empty = CtCircuit::empty(m);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params_kzg, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![root]]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params_kzg, &pk, &[circuit.clone()], instances, OsRng, &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], root: Fr, m: usize) -> Result<bool> {
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
    let empty = CtCircuit::empty(m);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params_kzg.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![root]]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params, &vk, instances, &mut transcript,
    ))
}

pub fn run_claimtree() -> Result<()> {
    let leaves: Vec<Fr> = (0..4).map(|i| Fr::from(1001 + i as u64)).collect();
    let m = leaves.len();
    let root = full_tree_root(&leaves);

    let circuit = CtCircuit { leaves: leaves.iter().map(|l| Value::known(*l)).collect() };

    eprintln!("proving Poseidon-Merkle claim root over M={m} leaves …");
    let proof = prove(&circuit, root, m)?;
    ensure!(verify(&proof, root, m)?, "verify failed");
    println!("✓ claim-tree proof verified — root is the Poseidon-Merkle root of the committed leaves");
    println!("  claimRoot = {}", hex::encode(pruv_circuits::circuit_params::fr_to_bytes(root)));
    println!("  proof_bytes = {} B  (k={K})", proof.len());

    ensure!(!verify(&proof, root + Fr::from(1u64), m)?, "wrong root verified");
    println!("  negative (wrong root)  : rejected ✓");
    Ok(())
}

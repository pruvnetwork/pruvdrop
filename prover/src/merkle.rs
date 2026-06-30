//! Sound in-circuit Merkle inclusion, built on the constrained Poseidon permutation.
//!
//! Chains `DEPTH` sound Poseidon hashes from a leaf up to the root: at each level
//! `parent = Poseidon(left, right)` where `(left,right) = bit ? (sib,cur) : (cur,sib)`
//! (matches pruv `merkle_root_from_path`). Levels are linked by **copy constraints**
//! (a level's parent cell IS the next level's input), so the chain is sound — unlike
//! PRUV's `merkle` circuit, which leaves the Poseidon outputs unconstrained.
//!
//! Public (instance): `[root, leaf]`.  Run:  cargo run --release -- --merkle-gadget

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

const MERKLE_K: u32 = 12;

#[derive(Clone)]
pub struct MerkleConfig {
    s: [Column<Advice>; 3],
    ark: [Column<Fixed>; 3],
    mds: [[Column<Fixed>; 3]; 3],
    cur: Column<Advice>,
    sib: Column<Advice>,
    bit: Column<Advice>,
    q_full: Selector,
    q_part: Selector,
    q_mux: Selector,
    instance: Column<Instance>,
}

#[derive(Clone)]
pub struct MerkleCircuit {
    pub leaf: Value<Fr>,
    pub siblings: Vec<Value<Fr>>,
    pub bits: Vec<Value<bool>>,
    pub depth: usize,
}

impl MerkleCircuit {
    pub fn empty(depth: usize) -> Self {
        Self {
            leaf: Value::unknown(),
            siblings: vec![Value::unknown(); depth],
            bits: vec![Value::unknown(); depth],
            depth,
        }
    }
}

impl Circuit<Fr> for MerkleCircuit {
    type Config = MerkleConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::empty(self.depth)
    }

    fn configure(meta: &mut ConstraintSystem<Fr>) -> MerkleConfig {
        let s = [meta.advice_column(), meta.advice_column(), meta.advice_column()];
        let cur = meta.advice_column();
        let sib = meta.advice_column();
        let bit = meta.advice_column();
        for c in [s[0], s[1], s[2], cur] {
            meta.enable_equality(c);
        }
        let ark = [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()];
        let mds = [
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
            [meta.fixed_column(), meta.fixed_column(), meta.fixed_column()],
        ];
        let instance = meta.instance_column();
        meta.enable_equality(instance);
        let q_full = meta.selector();
        let q_part = meta.selector();
        let q_mux = meta.selector();

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

        // mux: place [0, left, right] into the state at row 0 of each level
        meta.create_gate("mux", |meta| {
            let q = meta.query_selector(q_mux);
            let s0 = meta.query_advice(s[0], Rotation::cur());
            let s1 = meta.query_advice(s[1], Rotation::cur());
            let s2 = meta.query_advice(s[2], Rotation::cur());
            let cur = meta.query_advice(cur, Rotation::cur());
            let sib = meta.query_advice(sib, Rotation::cur());
            let bit = meta.query_advice(bit, Rotation::cur());
            let one = Expression::Constant(Fr::from(1u64));
            vec![
                q.clone() * bit.clone() * (bit.clone() - one),               // bit ∈ {0,1}
                q.clone() * s0,                                              // capacity lane = 0
                q.clone() * (s1 - (cur.clone() + bit.clone() * (sib.clone() - cur.clone()))), // left
                q.clone() * (s2 - (sib.clone() + bit * (cur - sib))),        // right
            ]
        });

        MerkleConfig { s, ark, mds, cur, sib, bit, q_full, q_part, q_mux, instance }
    }

    fn synthesize(&self, config: MerkleConfig, mut layouter: impl Layouter<Fr>) -> Result<(), ErrorFront> {
        let p = params();
        let half = p.full_rounds / 2;
        let all = p.full_rounds + p.partial_rounds;

        let mut current: Option<AssignedCell<Fr, Fr>> = None;
        let mut leaf_cell_for_instance: Option<AssignedCell<Fr, Fr>> = None;

        for level in 0..self.depth {
            let sib_val = self.siblings[level];
            let bit_val = self.bits[level];
            let cur_val: Value<Fr> = match &current {
                None => self.leaf,
                Some(c) => c.value().copied(),
            };
            let left = cur_val.zip(sib_val).zip(bit_val).map(|((c, s), b)| if b { s } else { c });
            let right = cur_val.zip(sib_val).zip(bit_val).map(|((c, s), b)| if b { c } else { s });
            let trace = left.zip(right).map(|(l, r)| perm_trace(l, r));
            let prev = current.clone();

            let (parent, leaf_opt) = layouter.assign_region(
                || format!("level_{level}"),
                |mut region| {
                    // fixed constants + round selectors
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
                    // mux row 0
                    config.q_mux.enable(&mut region, 0)?;
                    region.assign_advice(|| "sib", config.sib, 0, || sib_val)?;
                    region.assign_advice(|| "bit", config.bit, 0, || bit_val.map(|b| if b { Fr::from(1u64) } else { Fr::from(0u64) }))?;
                    let (cur_cell, leaf_opt) = match &prev {
                        None => {
                            let c = region.assign_advice(|| "cur(leaf)", config.cur, 0, || self.leaf)?;
                            (c.clone(), Some(c))
                        }
                        Some(pc) => (pc.copy_advice(|| "cur(parent)", &mut region, config.cur, 0)?, None),
                    };
                    let _ = cur_cell;

                    // state rows 0..=all from the permutation trace
                    let mut last_s0 = None;
                    for r in 0..=all {
                        for i in 0..3 {
                            let v = trace.clone().map(|t| t[r][i]);
                            let cell = region.assign_advice(|| "s", config.s[i], r, || v)?;
                            if r == all && i == 0 {
                                last_s0 = Some(cell);
                            }
                        }
                    }
                    Ok((last_s0.unwrap(), leaf_opt))
                },
            )?;

            if let Some(lc) = leaf_opt {
                leaf_cell_for_instance = Some(lc);
            }
            current = Some(parent);
        }

        layouter.constrain_instance(current.unwrap().cell(), config.instance, 0)?; // root
        layouter.constrain_instance(leaf_cell_for_instance.unwrap().cell(), config.instance, 1)?; // leaf
        Ok(())
    }
}

// ─── native tree helpers ────────────────────────────────────────────────────────

fn full_tree(leaves: &[Fr]) -> Vec<Vec<Fr>> {
    let mut levels = vec![leaves.to_vec()];
    while levels.last().unwrap().len() > 1 {
        let cur = levels.last().unwrap().clone();
        let next: Vec<Fr> = cur.chunks(2).map(|pair| perm_native(pair[0], pair[1])).collect();
        levels.push(next);
    }
    levels
}

fn path_for(levels: &[Vec<Fr>], mut idx: usize) -> (Vec<Fr>, Vec<bool>) {
    let mut sibs = Vec::new();
    let mut bits = Vec::new();
    for level in &levels[..levels.len() - 1] {
        sibs.push(level[idx ^ 1]);
        bits.push(idx & 1 == 1);
        idx >>= 1;
    }
    (sibs, bits)
}

// ─── prove / verify ─────────────────────────────────────────────────────────────

pub fn prove(circuit: &MerkleCircuit, root: Fr, leaf: Fr, depth: usize) -> Result<Vec<u8>> {
    use halo2_proofs::{
        halo2curves::bn256::Bn256,
        plonk::{create_proof, keygen_pk, keygen_vk},
        poly::kzg::{commitment::KZGCommitmentScheme, multiopen::ProverGWC},
        transcript::{Blake2bWrite, Challenge255, TranscriptWriterBuffer},
    };
    use rand::rngs::OsRng;

    let params_kzg = pruv_circuits::srs::get(MERKLE_K)?;
    let empty = MerkleCircuit::empty(depth);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let pk = keygen_pk(&*params_kzg, vk, &empty).map_err(|e| anyhow!("keygen_pk: {e:?}"))?;

    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![root, leaf]]];
    let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);
    create_proof::<KZGCommitmentScheme<Bn256>, ProverGWC<_>, _, _, _, _>(
        &*params_kzg,
        &pk,
        &[circuit.clone()],
        instances,
        OsRng,
        &mut transcript,
    )
    .map_err(|e| anyhow!("create_proof: {e:?}"))?;
    Ok(transcript.finalize())
}

pub fn verify(proof: &[u8], root: Fr, leaf: Fr, depth: usize) -> Result<bool> {
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

    let params_kzg = pruv_circuits::srs::get(MERKLE_K)?;
    let empty = MerkleCircuit::empty(depth);
    let vk = keygen_vk(&*params_kzg, &empty).map_err(|e| anyhow!("keygen_vk: {e:?}"))?;
    let verifier_params: ParamsVerifierKZG<Bn256> = params_kzg.verifier_params().clone();
    let instances: &[Vec<Vec<Fr>>] = &[vec![vec![root, leaf]]];
    let mut transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(proof);
    Ok(verify_proof_multi::<KZGCommitmentScheme<Bn256>, VerifierGWC<_>, _, _, SingleStrategy<_>>(
        &verifier_params,
        &vk,
        instances,
        &mut transcript,
    ))
}

pub fn run_merkle_test() -> Result<()> {
    let depth = 4usize;
    let n = 1usize << depth;
    let leaves: Vec<Fr> = (0..n).map(|i| Fr::from(1000 + i as u64)).collect();
    let levels = full_tree(&leaves);
    let root = *levels.last().unwrap().first().unwrap();
    let idx = 5usize;
    let (sibs, bits) = path_for(&levels, idx);

    let circuit = MerkleCircuit {
        leaf: Value::known(leaves[idx]),
        siblings: sibs.iter().map(|s| Value::known(*s)).collect(),
        bits: bits.iter().map(|b| Value::known(*b)).collect(),
        depth,
    };

    eprintln!("proving sound in-circuit Merkle inclusion (depth {depth}) …");
    let proof = prove(&circuit, root, leaves[idx], depth)?;
    ensure!(verify(&proof, root, leaves[idx], depth)?, "verify failed");
    println!("✓ in-circuit Merkle inclusion verified — sound Poseidon chain, depth {depth}");
    println!("  leaf #{idx} ∈ committed root");
    println!("  proof_bytes = {} B  (k={MERKLE_K})", proof.len());

    ensure!(!verify(&proof, root, leaves[idx] + Fr::from(1u64), depth)?, "wrong leaf verified");
    println!("  negative (wrong leaf) rejected ✓");
    Ok(())
}

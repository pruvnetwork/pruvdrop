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

use anyhow::{ensure, Result};
use halo2_proofs::halo2curves::bn256::Fr;

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

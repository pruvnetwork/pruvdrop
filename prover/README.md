# pruvdrop-prover (Phase 1)

Real ZK inclusion prover for pruvdrop, built on **PRUV's Halo2 KZG/BN254 circuits**.
Implements **Phase 1** of [`../docs/ZK-ALLOCATION-PROOF.md`](../docs/ZK-ALLOCATION-PROOF.md).

## What it proves

Given the winner allocation, it builds a **Poseidon (BN254) Merkle tree** (depth 20)
over the winner leaves `Poseidon(pk_hash, amount)` and produces a real
`pruv-circuits` Halo2 inclusion proof that a chosen winner's leaf is in that tree.
It then writes `proof_hash = SHA-256(proof_bytes)` — the 32-byte artifact PRUV's
attestation records on-chain.

> **Scope (honest):** Phase 1 proves *winner ∈ committed Poseidon tree* — a parallel
> commitment to the SHA-256 claim tree the on-chain claim program uses. It does **not**
> prove the allocation *function* (scoring → selection → payout) is correct. That is
> **Phase 2** (the selection/payout circuit). Do not advertise "ZK-fair" off Phase 1.

## Setup (local dev)

Requires a side-by-side checkout of the private PRUV repo (the `path` dependency in
`Cargo.toml`):

```
~/Desktop/pruv-solana-main/      # private pruv (provides pruv-circuits)
~/Desktop/pruvdrop/prover/       # this crate
```

CI / operators without the side-by-side checkout: switch the dependency to the `git`
form in `Cargo.toml` and provide a read-only deploy key for `pruvnetwork/pruv`.

## Build & run

```bash
cd prover
# first build fetches halo2 from git (slow); a dev KZG SRS is generated automatically.
# for production, set a real powers-of-tau: export PRUV_SRS_PATH=/path/to/hermez.ptau
cargo run --release -- \
  --claims ../app/out/claims.json \
  --index 0 \
  --out ../web/public
```

Outputs into `web/public/`:

- `allocation-proof.json` — `{ scheme, index, wallet, leaf, poseidon_root, proof_hash,
  public_inputs, proof_b64, note }`
- `allocation-proof.bin` — raw Halo2 proof bytes

The `/verify` page can then surface `poseidon_root` + `proof_hash`, and (once
`pruv-attestation` is wired) the on-chain attestation that records the same hash.

## Pipeline position

```
campaign(:x) → allocate → claimtree → build-campaign → [pruvdrop-prover] → attest (pruv) → deploy
```

## Next (Phase 2 / 3)

- **Phase 2:** allocation-correctness circuit (threshold + permutation argument, not
  in-circuit sorting) proving `inputRoot + seed + rules ⟹ claimRoot`.
- **Phase 3:** gate the claim program on a matching `proof_hash` + pruv attestation PDA.

# pruvdrop × PRUV — ZK Allocation Proof (plan)

> Status: **design / not yet implemented.** This document is the engineering plan for
> upgrading pruvdrop's fairness model from *transparent recompute* to a **real
> zero-knowledge proof of correct allocation**, reusing PRUV's existing Halo2 stack.
> Do **not** advertise "ZK" on the site until Phase 2 ships a real, verifiable proof.

## 0. Why (and why not)

pruvdrop today is **transparent-verifiable**, not zero-knowledge:

- the candidate set + scores are public,
- the allocation Merkle root is committed on-chain **before** the draw seed,
- the rule is deterministic → anyone can **recompute** the root and check it matches.

That is the *right default* for an airdrop — you **want** winners public. ZK adds value
in exactly one place:

- **Succinct verification at scale.** "Recompute it yourself" means re-pulling every
  post and re-scoring thousands–millions of candidates. A ZK proof lets anyone confirm
  *"the committed `claimRoot` is the correct output of the published rules applied to the
  committed input set"* **without** redoing the work — and lets the **claim program**
  gate payouts on an attested-correct allocation.

ZK does **not** add privacy here (inputs are public by design) and is **not** needed for
fairness (commitment + determinism already deliver it). So this is a **scale + on-chain
enforceability** upgrade, not a fairness fix.

## 1. What PRUV already gives us (reuse, don't rebuild)

From `pruv-solana-main`:

| Component | Where | Reuse |
|---|---|---|
| Halo2 UltraPLONK, **KZG/BN254** backend (PSE v0.4) | `circuits/Cargo.toml` | proving system |
| **Poseidon-128** native + in-circuit (Circom-compatible) | `circuits/src/poseidon_hasher.rs` | leaf/tree hashing |
| **Merkle inclusion circuit** (depth 20) | `circuits/src/merkle.rs` | membership of input/winner leaves |
| Nullifier + leaf-commitment pattern | `circuits/src/governance_vote.rs` | per-recipient binding |
| SRS loader/cache, proving-key cache, **batch prover (Rayon)** | `circuits/src/{srs,proving_key_cache,batch_prover}.rs` | performance / scale |
| **Attestation model** — off-chain proof, node Ed25519 quorum, on-chain `proof_hash = SHA-256(proof)` | `programs/pruv-attestation/src/lib.rs` | **on-chain verification path** |

**Key architectural inheritance:** PRUV verifies proofs **off-chain** and records only
`proof_hash` (32 B) on-chain, with a quorum of node signatures attesting they checked it.
So pruvdrop **does not** need a Halo2 verifier on Solana (no `alt_bn128` pairing gymnastics).
We follow the same path.

## 2. The statement to prove

Let the allocation be committed in two roots:

- `inputRoot` = Merkle root over candidate leaves `Lᵢ = Poseidon(wallet_i, score_i)`,
  committed **before** the seed (extends the current `commit_snapshot`).
- `claimRoot` = Merkle root over winner leaves `Wⱼ = Poseidon(index_j, wallet_j, amount_j)`
  (this is the existing claim tree).

**Public inputs (instance):** `inputRoot`, `claimRoot`, `seed` (slot-hash, revealed after
commit), and rule constants `{ N, pot, weights, mode }`.

**Private witness:** the full candidate vector `(wallet_i, score_i)`, the selected winners,
and their amounts.

**Proven:**
1. **Consistency** — the witnessed candidates hash to `inputRoot`.
2. **Selection** — the winners are exactly those the published rule selects:
   - *top-N mode*: the N highest `score_i` (deterministic tie-break by `wallet`);
   - *weighted-lottery mode*: the draws of a `seed`-seeded PRNG over the cumulative-weight
     array select exactly these winners.
3. **Payout** — winner `amount_j` equals the rule's payout (equal split / weight-proportional),
   and the winners hash to `claimRoot`.

## 3. Circuit design

### The hard part — and the cheap way around it

Proving (2) naïvely means **sorting N candidates in-circuit** (expensive). Avoid it:

- **top-N via threshold + permutation argument.** Witness a threshold `t` and a permutation
  π. Prove (a) every winner has `score ≥ t`, (b) a counting argument shows **exactly N**
  candidates satisfy `score ≥ t` (with deterministic tie-break at the boundary), (c) π maps
  winners↔input leaves (a standard PLONK permutation/lookup, no sorting network). Cost is
  **linear** in the set size, not `N log N` sorting.
- **weighted-lottery via prefix sums.** Witness the cumulative-weight array; prove each
  PRNG draw `dₖ = H(seed, k) mod Σw` lands in winner j's `[prefix_{j-1}, prefix_j)` bucket.
  Range checks + a lookup; also linear.

Either way the circuit **processes the whole committed set once** → proof size is constant,
verifier cost is constant; **prover** cost scales with set size.

### Scale strategy (PRUV's batch/recursion)

- ≤ ~4–8k candidates: single circuit, `k ≈ 17–19`, proved with the cached proving key.
- larger: **chunk + recurse** — `batch_prover.rs` proves per-chunk sub-statements (each chunk's
  partial count / partial root) in parallel (Rayon), then a small aggregation circuit folds the
  chunk results into the final `(inputRoot, claimRoot)` statement. Keeps any single proof small.

### Reused gadgets

`poseidon_hasher` (leaves + tree), `merkle` (root recomputation in-circuit), nullifier pattern
from `governance_vote` (optional per-winner binding). New code = the **selection/counting chip**
and the **payout chip**.

## 4. Verification & on-chain binding (PRUV attestation model)

1. **Prover (operator / pruv node), off-chain:** after `build-campaign`, run the allocation
   circuit → `proof_bytes` (~1 KB) + `proof_hash = SHA-256(proof_bytes)`.
2. **Publish proof** off-chain: `web/public/allocation-proof.bin` (+ IPFS/Arweave pin), and add
   `proofHash`, `inputRoot`, `seed`, `srsId`, `vkId` to `config.json`.
3. **Attest on-chain:** reuse `pruv-attestation::submit_attestation` — pruv nodes verify the
   proof off-chain and gossip Ed25519 sigs; a quorum + `proof_hash` is written on-chain.
4. **(Optional) gate claims:** extend the claim program's `initialize` to store `proof_hash` and
   require a matching pruv attestation PDA before `claim` opens → payouts are **bound to an
   attested-correct allocation**, not just an un-explained root.
5. **Anyone verifies:** fetch `allocation-proof.bin`, run `pruv_circuits::allocation::verify`
   against the public inputs in `config.json`; check `SHA-256` matches the on-chain `proof_hash`
   and the attestation quorum.

No Solana-side pairing verifier required.

## 5. Integration into pruvdrop

- **Pipeline:** new step `app/src/prove-allocation.ts` → shells into a small Rust binary
  (`pruv-circuits`-backed) that reads `out/{candidates,allocation,claims}.json` + `seed` and
  emits `allocation-proof.bin` + public inputs. Wire after `build-campaign`.
- **Config:** `config.json` gains `proofHash`, `inputRoot`, `seed`, `srsId`, `vkId`,
  `proofUri`.
- **/verify page:** add a "Zero-knowledge proof" section — show `proofHash`, the attestation
  PDA + signer quorum (Solscan link), public inputs, and a **"verify the proof"** button
  (WASM-compiled `verify`, or a hosted verify endpoint) that re-checks `proof_bytes` in the
  browser and confirms the hash matches on-chain.
- **Claim program (optional, Phase 3):** `proof_hash` field + attestation CPI check.

## 6. Phased roadmap

| Phase | Deliverable | Effort | Value |
|---|---|---|---|
| **0** (done) | transparent recompute + commit-before-seed | — | fairness baseline |
| **1 — inclusion** | reuse `merkle.rs`: per-winner ZK inclusion proof vs committed `claimRoot`; `proof_hash` on-chain via attestation; `/verify` shows it | ~days | real proof shipped (modest claim: "winner ∈ committed tree") |
| **2 — allocation-correctness** | the §3 selection/payout circuit (`inputRoot`+`seed`+rules ⟹ `claimRoot`); off-chain verify + attestation; `/verify` "verify the proof" | ~weeks (real ZK eng) | **the differentiator** — succinct proof the whole allocation is correct |
| **3 — on-chain gating** | claim program requires matching `proof_hash` + pruv attestation | ~days on top of 2 | payouts bound to attested-correct allocation |

## 7. Honest caveats

- **Don't ship the "ZK" badge before Phase 2.** Phase 1 only proves *inclusion* in a tree we
  already commit — say exactly that, not "provably-fair-by-ZK".
- **Prover scale.** Set-size-linear prover; very large campaigns need the chunk+recurse path.
  Benchmark before promising mainnet-scale.
- **Trusted setup.** KZG SRS (use a real Hermez/perpetual-powers-of-tau `ptau`, not the dev
  insecure SRS in `srs.rs`); pin `srsId`.
- **Audit.** The selection/counting chip is new and security-critical — audit before mainnet
  funds ride on Phase 3 gating.
- **Attestation trust.** On-chain trust = the pruv node quorum that signs. Document the node
  set + threshold; that is the security model, not native on-chain verification.

## TL;DR

Reuse PRUV (Halo2 KZG/BN254 + Poseidon + Merkle + **attestation** = no on-chain verifier
needed). Ship **Phase 1** (inclusion proof) quickly as a real ZK artifact, then build the
**Phase 2** allocation-correctness circuit (threshold + permutation, not in-circuit sorting) —
that is the genuine "verify the whole draw without recomputing it" differentiator, and it is
exactly PRUV's thesis.

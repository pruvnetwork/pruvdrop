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

## 6b. Phase 2 — locked design

Decisions (agreed): **unify on Poseidon** end-to-end, **top-N mode first**, **spike before full build**.

**Poseidon unification.** Migrate the claim tree + on-chain claim verify from SHA-256 to
Poseidon (Light-Protocol-style: ~20 Poseidon hashes per claim path in BPF). Required so
`inputRoot`, `claimRoot`, and the circuit all use one cheap-in-circuit hash. (SHA-256
in-circuit is ~thousands of rows/hash; Poseidon is cheap.)

**Top-N statement (no in-circuit sorting).** Prove "exactly N candidates have score ≥ t"
with a threshold + counting argument:
- per candidate `i`: a comparator `b_i = (score_i ≥ t)` proven via bit-decomposition of
  `score_i − t + 2^B` (the MSB is the result); `b_i` boolean.
- running sum `Σ b_i = N` (public). Tie-break at the boundary by wallet order.
- a permutation/lookup binds the N winners to N distinct committed input leaves.
Cost is **O(M)** (M = candidate count), dominated by re-hashing `inputRoot` in-circuit.

**Spike (de-risk first).** A minimal circuit proving just the novel mechanic —
`Σ_{i<M} (score_i ≥ t) = N` over witnessed scores, comparator via bit-decomposition,
`t` and `N` public — on small M. No Poseidon, no payout (those are de-risked elsewhere).
Validates the comparator + counting gates and measures `k` / prove time before the full
build. Lives in `prover/src/topn.rs`, run with `pruvdrop-prover --spike`.

After the spike: add Poseidon input-tree consistency, the payout chip, `claimRoot` output,
then weighted-lottery mode, then recursion for large M.

## 7. Honest caveats

- **⚠️ PRUV's `PoseidonChip` is currently unsound (must fix before any soundness claim).**
  `PoseidonChip::configure` allocates `in_a/in_b/out` advice columns with equality but
  **no gate enforcing `out == Poseidon(in_a, in_b)`** — the hash output is assigned as a
  *free* witness (the native value). So `merkle`, `governance_vote`, and therefore the
  **Phase-1 inclusion proof** are only sound for an *honest* prover; a malicious prover can
  forge hash steps (e.g. fake an inclusion). The real S-box gate (`SboxChip`) exists but is
  not wired into a full Poseidon permutation. **Fix — built:** a sound, fully-constrained
  Poseidon-128 permutation now exists in `prover/src/poseidon.rs` (`--poseidon-gadget`):
  every ARK / S-box (x⁵, degree-5 gate) / MDS step is constrained against light-poseidon's
  exact width-3 constants, it matches `hash_two`, and a wrong output is rejected (1344 B,
  k=11). The Poseidon gadget is chained into a **sound in-circuit Merkle inclusion** in
  `prover/src/merkle.rs` (`--merkle-gadget`): levels linked by copy constraints, `leaf ∈ root`
  proven, wrong leaf rejected (depth-4 demo, 1536 B, k=12). **Upstreamed:** the sound
  permutation now lives in the protocol repo as `pruv-circuits::poseidon_sound`
  (`circuits/tests/test_poseidon_sound.rs` passes: native == `hash_two`, prove/verify binds the
  output, a wrong output is rejected). It is **additive / opt-in** — committed to pruv-circuits
  without touching the existing circuits. A prototype that wires it into `merkle` + `governance`
  was built and tested (`test_merkle`/`test_governance` passed) but **deliberately not kept**:
  rewriting those core circuits changes their verification keys and invalidates existing proofs,
  so that adoption is left to the PRUV team. **Therefore `merkle`/`governance` and the pruvdrop
  Phase-1 inclusion proof remain honest-prover-only until the team wires `poseidon_sound` in.**
- **Sound counting over a committed set — built.** `prover/src/combined.rs` (`--combined`)
  binds the scores to a sound **Poseidon-chain** commitment `C` (each step the constrained
  permutation, score cells shared with the comparator by copy constraints) and proves exactly
  N are ≥ t over that committed set. Both a false count and a swapped score are rejected — the
  RLC fingerprint of the earlier spike is now replaced by a real Poseidon binding. (`topn.rs`
  keeps the RLC version for reference.) Production can swap the chain for a Poseidon-Merkle root
  — the gadget exists in `merkle.rs` — for cheap on-chain inclusion.
- **Top-N winners by identity — built.** `prover/src/selection.rs` (`--selection`) adds the
  wallet: `inputC` commits `(wallet_i, score_i)` per candidate, `selected_i = win_i·wallet_i`
  (a mux gate), and `claimC` commits the winner wallets. Proves the winners are *exactly* the
  top-N, bound to identity; a wrong N and a swapped winner wallet are both rejected.
- **Payout with conservation — built.** `prover/src/payout.rs` (`--payout`) adds real amounts:
  a mask gate forces `amount_i·(1−win_i)=0` (losers get 0), a running sum constrains
  `Σ amount_i = pot` (public), and `claimC` commits `(wallet_i, amount_i)` — the claim-tree
  contents. Verified with proportional amounts `[3750,0,6250]` summing to pot; a wrong pot and
  paying a loser are both rejected. The amount *values* are the operator's published split
  (∝ score, re-checkable from public scores); **remaining:** recursion for scale, then upstream
  the gadgets into pruv-circuits + a real SRS + audit.
- **Proportional payout — proven.** `prover/src/proportional.rs` (`--proportional`) makes the
  amounts provably correct, not trusted: for each winner it proves `pot·score_i = amount_i·S +
  rem_i` with `0 ≤ rem_i < S` and `amount_i` range-bounded (so `amount_i = ⌊pot·score_i/S⌋` over
  the integers, no field wraparound), `S = Σ score_i` in-circuit, and `Σ amount + dust = pot`
  with tiny bounded dust. A tampered (non-floor) amount has no valid `rem` and is rejected.
  This closes the last soundness gap — the payout *rule* is now provable, not just conservation.
- **Poseidon-Merkle claim root — built.** `prover/src/claimtree.rs` (`--claimtree`) recomputes a
  Poseidon Merkle tree over the M claim leaves bottom-up (each node the constrained permutation,
  children copy-linked) and binds the top to the public `claimRoot`; a wrong root is rejected.
  A Merkle root (not the chain) lets the on-chain claim program verify each recipient with an
  O(log M) Poseidon path — the bridge to Phase-3 on-chain gating.
- **Weighted-lottery (anti-whale) — built.** `prover/src/lottery.rs` (`--lottery`) gives each
  candidate a cumulative-weight bucket `[prefix[j], prefix[j]+score[j])` whose width is its
  score, and proves each public draw `r_k` lands in exactly one bucket
  (`Σ_j [prefix[j] ≤ r_k < prefix[j]+score[j]] = 1`, prefix sums constrained). So selection
  probability ∝ score and a small account can still win — an alternative to top-N. Draws are
  public here; in production `r_k = Poseidon(seed,k) mod S` over a commit-before-reveal slot
  hash (the bounded-mod reduction is the documented add-on). A wrong S is rejected.
- **Boundary tie-break — built.** `prover/src/tiebreak.rs` (`--tiebreak`) orders candidates by
  `(score desc, index asc)`: `win_i = [score_i>t_s] OR ([score_i==t_s] AND [i≤t_idx])`, using an
  is-zero gadget for the equality and a small index comparator for the tie. Picks exactly N
  through a 3-way boundary tie (three equal scores); a wrong N and a shifted boundary are
  rejected. Folds into the allocation comparator so ties no longer block a clean threshold.
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

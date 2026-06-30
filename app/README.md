# @pruv/viral-airdrop

Farcaster-first **verifiable viral airdrop**: airdrop to the people with the most viral casts carrying a coin tag — provably fairly, allocated by PRUV.

> Built in response to Ansem's request for a viral-tag airdrop tool. Farcaster-first so there is **no paid X API**, **wallet mapping is free** (verified addresses), and **sybil farming is costlier**.

## Pipeline

```
[1] Ingest (Farcaster/Neynar)   src/ingest.ts    casts with the tag + engagement + verified Solana wallet
[2] Score + filter              src/score.ts     virality scoring, reach-dampening, quality/sybil gates
[3] Snapshot + Merkle commit    src/snapshot.ts  canonical (fid-sorted) set + SHA-256 Merkle root
   ── commit root on-chain BEFORE seed (PRUV commit-before-seed) ──
[4] Allocation                  src/allocate.ts  top-N (deterministic) OR weighted-lottery (slot-hash seed)
[5] Merkle-drop claim           src/claim-tree.ts + programs/viral-airdrop-claim (Anchor)
```

All five layers are **implemented and self-tested**:
`npm run selftest` · `selftest:alloc` · `selftest:claim`, plus the on-chain program
`cargo test -p viral-airdrop-claim` (proves the Rust leaf/node hashing is byte-for-byte
identical to `claim-tree.ts`). Allocation reuses PRUV's verifiable-seed source
(`fetchSlotHashForSlot` in `../sdk`).

### Claim flow (Layer 5)
```
npx tsx src/run-claimtree.ts out/allocation.json   # -> out/claims.json (root + per-recipient proofs)
```
On-chain (`programs/viral-airdrop-claim`):
- `initialize(merkle_root)` — authority creates the Distributor PDA + vault, then funds the vault.
- `claim(index, amount, proof)` — caller proves their leaf; a per-index ClaimStatus PDA (`init`)
  is the nullifier (one claim per recipient); tokens move vault → claimant.

## Run

```bash
# offline logic check (no API key needed)
npm run selftest

# real campaign
cp .env.example .env   # add NEYNAR_API_KEY (free at neynar.com)
cp campaign.example.json campaign.json   # set query + window (unix seconds)
NEYNAR_API_KEY=xxx npx tsx src/run-campaign.ts ./campaign.json
# -> out/candidates.json, out/snapshot.json (commit snapshot.merkleRoot on-chain)
```

## Fairness model

- **Allocation** (given the snapshot): fully verifiable, zero operator trust — PRUV.
- **Snapshot integrity**: the candidate set is committed on-chain **before** the seed slot, and
  published publicly. Because Farcaster data is open, anyone can re-pull and recompute the Merkle
  root — the operator cannot cherry-pick after seeing the seed.
- **Sybil**: Neynar quality-score gate + follower floor + reach-dampening + optional per-author cap.
  Not perfect; methodology is public and challengeable.

## Config (`campaign.json`)

| field | meaning |
|---|---|
| `query` | ticker/cashtag to search (e.g. `$PRUV`) |
| `windowStart` / `windowEnd` | campaign window, unix seconds |
| `minQualityScore` | Neynar user score gate (0–1); 0 disables |
| `minFollowers` | follower floor |
| `weights` | `{ like, recast, reply }` engagement weights |
| `reachExponent` | divide raw engagement by `followers^exponent` (anti-whale); 0 = off |
| `perAuthorCap` | clamp per-author score (0 = no cap) |
| `maxCasts` | ingestion safety cap |

## Next (Layers 4–5)
1. **Commit** `merkleRoot` via a PRUV on-chain instruction before the seed slot.
2. **Allocate**: reuse `deriveWinnerIndex(slotHash, ...)` for weighted-lottery, or pure deterministic
   top-N ranking (no seed needed) — both recomputable from the committed snapshot.
3. **Distribute**: build a claim Merkle tree keyed on `(wallet, amount)` and a Solana claim program
   with a per-recipient nullifier (gas-efficient Merkle-drop).

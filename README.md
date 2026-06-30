# pruvdrop

**Verifiable viral airdrop on Solana** — airdrop to the people with the most viral
Farcaster casts carrying a coin tag, *provably fairly*. Allocation is committed
before the randomness is known and is independently recomputable, so no one can
claim the operator rigged it for insiders.

Farcaster-first: **no paid X API**, wallet mapping is free (verified addresses),
and sybil farming is costlier. Built to slot onto PRUV's verifiable-allocation model.

## Pipeline

```
[1] Ingest (Farcaster/Neynar)   app/src/ingest.ts      casts + engagement + verified Solana wallet
[2] Score + filter              app/src/score.ts       virality, reach-dampening, quality/sybil gates
[3] Snapshot + Merkle commit    app/src/snapshot.ts    canonical set + SHA-256 root
   ── commit root on-chain BEFORE the seed slot (commit-before-seed) ──
[4] Allocate                    app/src/allocate.ts    top-N (deterministic) OR weighted-lottery (slot-hash seed)
[5] Claim tree + on-chain claim app/src/claim-tree.ts  + programs/viral-airdrop-claim (Anchor)
[6] Claim portal                app/portal/            static site; recipients claim with their wallet
```

Every layer is self-tested; the on-chain program is verified live (claim, nullifier,
bad-proof rejection, commit-before-seed, end-to-end lottery seed, and the portal's
exact claim transaction).

## Live on devnet

| | |
|---|---|
| Program | `3oCMjxiXMorGrFUrFqYUmpfwG1FMLaLBWJBh6pVRcLqJ` |
| Network | devnet (BPF Upgradeable Loader) |

(The committed `app/portal/config.json` + `claims.json` are a devnet demo built from
a real 33-author Farcaster snapshot.)

## Layout

```
programs/viral-airdrop-claim/   Anchor program: initialize, commit_snapshot, claim (+ nullifier)
app/src/                        TS pipeline (ingest → score → snapshot → allocate → claim tree)
app/portal/                     static claim portal (Phantom)
tests/                          on-chain integration test
```

## Quick start

```bash
# program
anchor build -p viral_airdrop_claim
cargo test -p viral-airdrop-claim         # TS↔Rust hash parity

# pipeline
cd app && npm install
npm run selftest && npm run selftest:alloc && npm run selftest:claim

# real campaign (needs a free Neynar API key)
NEYNAR_API_KEY=xxx npm run campaign -- ./campaign.json     # -> out/snapshot.json
npm run allocate                                           # -> out/allocation.json
npm run claimtree                                          # -> out/claims.json
MINT=<mint> RPC=https://api.devnet.solana.com npm run portal   # -> portal/config.json + claims.json
npx serve portal
```

## Deploy notes

- **Local validator** (solana 3.x): `solana program-v4 deploy` (LoaderV4).
- **Devnet** (LoaderV4 unsupported): use solana 2.2 + `solana program deploy --use-rpc` (LoaderV3).
- After building, run `anchor keys sync` (or set the program id) before deploying.

## Build gotcha (SBF + edition2024)

If `anchor build` fails with `block-buffer ... requires edition2024`, pin the
hashing chain back to an SBF-compatible version (the Solana platform-tools Cargo
predates edition2024):

```bash
cargo update -p blake3 --precise 1.5.5
```

The committed `Cargo.lock` already pins this.

## Eligibility — verify-to-qualify grace window

Only casters with a **Solana verified address** on their Farcaster profile can
receive (no claim-time wallet linking — keeps the on-chain model trustless).

To avoid silently excluding the ~30% of casters who are otherwise eligible but
have no Solana address, the ingest does **not** drop them — it collects them into
`out/pending.json` ("verify to qualify"):

```
[1] run-campaign  ->  eligible (have Solana wallet)  +  pending (no wallet yet)
[2] open a grace window: notify pending casters to verify a Solana address on Farcaster
[3] re-run run-campaign  ->  newly-verified wallets move from pending -> eligible
[4] finalize: commit snapshot.merkleRoot + allocate
```

This turns the exclusion into an opt-in and nudges casters to verify a Solana
wallet (an ecosystem win). Communicate it in the campaign:
*"verify your Solana wallet on Farcaster to qualify."*

## Platform: Farcaster (auto-scan) or X (claim-based)

Only Layer 1 is platform-specific; the rest is platform-agnostic.

- **Farcaster** (`run-campaign`): auto-scans casts via Neynar; free wallet mapping
  (verified addresses); best sybil resistance. Reaches the crypto-Farcaster crowd.
- **X / Twitter** (`run-campaign-x`): **claim-based, wallet-in-tweet**, no paid API.
  Casters tweet about the ticker AND include their Solana address in the tweet;
  the tweet URL/id is submitted; we read each via the free syndication endpoint
  and extract the ticker, engagement, and the embedded address. The address is in
  the tweet, so the binding is **trustless** (only the author can author it) — no
  OAuth or relayer needed.

```bash
# X campaign: tweets.json (URLs/ids) + campaign-x.json (query = ticker, minQualityScore 0)
npm run campaign:x -- ./tweets.json ./campaign-x.json   # -> out/snapshot.json + pending.json
```

X caveats: the syndication endpoint is unofficial — likes are reliable, but
retweet/reply counts and follower counts may be absent (treated as 0); sybil
resistance is weaker than Farcaster (no quality score). A tweet with the ticker
+ engagement but no Solana address becomes `pending` ("repost including your
Solana address to qualify"). Use X for reach (e.g. an X-native influencer's
audience); use Farcaster for the cleanest data + sybil resistance.

## Fairness

- **Allocation**: zero operator trust — deterministic from the committed snapshot
  (+ the on-chain slot-hash seed for lotteries), recomputable by anyone.
- **Snapshot integrity**: committed on-chain before the seed exists; Farcaster data
  is open, so anyone can re-pull and recompute the root.
- **Distribution**: Merkle-drop with a per-recipient nullifier (one claim each).

---

Built with PRUV (verifiable allocation). Program + pipeline + portal in one repo.

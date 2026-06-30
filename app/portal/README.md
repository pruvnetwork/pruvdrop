# Claim Portal

Static front-end for the verifiable viral airdrop. Recipients connect their wallet
and claim their award — proven against the on-chain Merkle root.

## Generate data

```bash
# after run-claimtree.ts produced out/claims.json:
MINT=<token-mint> RPC=https://api.devnet.solana.com \
  npx tsx src/run-portal.ts out/claims.json
# -> portal/config.json + portal/claims.json
```

## Serve

```bash
npx serve viral-airdrop/portal      # or host on Vercel / IPFS / any static host
```

## How it works

- Loads `config.json` (programId, rpc, mint, claimRoot) and `claims.json`
  (`wallet -> { index, amount, proof }`).
- Connects Phantom (`window.solana`).
- Looks up the connected wallet's award.
- On **Claim**, builds two instructions and sends them via the wallet:
  1. create the recipient's associated token account (idempotent),
  2. `claim(index, amount, proof)` on `viral-airdrop-claim`.
- The program verifies the Merkle proof against the on-chain root and pays out
  once (per-recipient nullifier PDA prevents double claims).

No backend. The portal is fully static; all trust is in the on-chain program and
the publicly committed Merkle root.

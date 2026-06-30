"use client";
import { useEffect, useState } from "react";

interface Cfg { programId: string; rpcUrl: string; mint: string; claimRoot: string; }

export default function Verify() {
  const [cfg, setCfg] = useState<Cfg | null>(null);
  const [count, setCount] = useState<number | null>(null);

  useEffect(() => {
    fetch("/config.json").then((r) => r.json()).then(setCfg).catch(() => {});
    fetch("/claims.json").then((r) => r.json()).then((c) => setCount(Object.keys(c).length)).catch(() => {});
  }, []);

  const cluster = cfg?.rpcUrl.includes("devnet") ? "?cluster=devnet" : "";
  const ex = (a: string) => `https://explorer.solana.com/address/${a}${cluster}`;

  return (
    <div className="page">
      <a className="back" href="/">← back</a>
      <div className="hero" style={{ paddingTop: 18 }}>
        <span className="pill">Proof</span>
        <h1>Verify the allocation</h1>
        <div className="lead">
          Fairness here isn&apos;t a promise — it&apos;s recomputable. Everything you need to
          check the winners yourself is public and anchored on-chain.
        </div>
      </div>

      <div className="section">
        <h2>On-chain commitment</h2>
        <div className="panel">
          <div className="kv"><span className="k">Claim Merkle root</span><span className="v mono">{cfg?.claimRoot ?? "…"}</span></div>
          <div className="kv"><span className="k">Distributor program</span><span className="v mono">{cfg ? <a href={ex(cfg.programId)} target="_blank">{cfg.programId} ↗</a> : "…"}</span></div>
          <div className="kv"><span className="k">Token mint</span><span className="v mono">{cfg ? <a href={ex(cfg.mint)} target="_blank">{cfg.mint} ↗</a> : "…"}</span></div>
          <div className="kv"><span className="k">Network</span><span className="v">{cfg?.rpcUrl.includes("devnet") ? "devnet" : "mainnet"}</span></div>
          <div className="kv"><span className="k">Recipients</span><span className="v">{count ?? "…"}</span></div>
          <div className="note">
            The recipient list is hashed into the Merkle root above and committed on-chain
            before any claim opens. A per-recipient nullifier means each address can claim once.
            The root cannot be edited after commitment — so the winner list is fixed and tamper-evident.
          </div>
        </div>
      </div>

      <div className="section">
        <h2>Public inputs</h2>
        <div className="panel">
          <div className="kv"><span className="k">Full allocation (wallet → amount + proof)</span><span className="v"><a href="/claims.json" target="_blank">claims.json ↗</a></span></div>
          <div className="kv"><span className="k">Campaign config</span><span className="v"><a href="/config.json" target="_blank">config.json ↗</a></span></div>
          <div className="note">
            Because submissions come from public posts (Farcaster / X), anyone can re-pull the
            same data independently — there is no private list only the operator can see.
          </div>
        </div>
      </div>

      <div className="section">
        <h2>Recompute it yourself</h2>
        <div className="panel">
          <div className="li"><span className="ic c">1</span><span>Pull the public posts carrying the campaign tag, with their engagement.</span></div>
          <div className="li"><span className="ic c">2</span><span>Apply the published scoring + allocation rules — deterministic, no operator discretion.</span></div>
          <div className="li"><span className="ic c">3</span><span>Build the claim Merkle tree from the result.</span></div>
          <div className="li"><span className="ic c">4</span><span>Check your root equals the on-chain root above. If it matches, the allocation was honest.</span></div>
          <div className="note">
            Open-source pipeline: <a href="https://github.com/pruvnetwork/pruvdrop" target="_blank">github.com/pruvnetwork/pruvdrop ↗</a>.
            For lottery-mode campaigns, the random seed is the hash of a future Solana slot,
            committed before it exists — so the operator cannot steer who wins.
          </div>
        </div>
      </div>
    </div>
  );
}

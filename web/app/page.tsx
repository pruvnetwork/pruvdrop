"use client";
import { useEffect, useState } from "react";

interface Cfg { programId: string; rpcUrl: string; mint: string; claimRoot: string; }

export default function Home() {
  const [cfg, setCfg] = useState<Cfg | null>(null);
  useEffect(() => { fetch("/config.json").then((r) => r.json()).then(setCfg).catch(() => {}); }, []);

  const cluster = cfg?.rpcUrl.includes("devnet") ? "?cluster=devnet" : "";
  const short = (s?: string) => (s ? `${s.slice(0, 6)}…${s.slice(-6)}` : "…");

  return (
    <div className="page">
      <div className="hero">
        <span className="pill">Solana · $ANSEM · provably fair</span>
        <h1>$ANSEM, distributed fairly.<br />And you can prove it.</h1>
        <div className="lead">
          No insider list. No free wallets. The most viral $ANSEM posts win — and the
          allocation is <b>committed on-chain before the draw</b>, so anyone can
          recompute the winners. Earned by the community, not handed to insiders.
        </div>
        <div className="cta">
          <a className="primary" href="/submit">Submit your tweet</a>
          <a className="ghost" href="/claim">Claim your airdrop</a>
        </div>
      </div>

      <div className="section">
        <h2>Why this is different</h2>
        <div className="grid2">
          <div className="panel bad-tint">
            <h3>How memecoin drops usually go</h3>
            <div className="li"><span className="ic x">✕</span><span>Supply concentrated in a few wallets</span></div>
            <div className="li"><span className="ic x">✕</span><span>Opaque allocation — trust the team</span></div>
            <div className="li"><span className="ic x">✕</span><span>Free allocations to insiders</span></div>
            <div className="li"><span className="ic x">✕</span><span>No way to check who got what, or why</span></div>
          </div>
          <div className="panel good-tint">
            <h3>How pruvdrop does it</h3>
            <div className="li"><span className="ic c">✓</span><span>Earned by virality — you post, you qualify</span></div>
            <div className="li"><span className="ic c">✓</span><span>Allocation committed on-chain before the draw</span></div>
            <div className="li"><span className="ic c">✓</span><span>Full recipient list is public</span></div>
            <div className="li"><span className="ic c">✓</span><span>Recompute the winners yourself</span></div>
          </div>
        </div>
      </div>

      <div className="section">
        <h2>Verify the fairness</h2>
        <div className="panel">
          <h3>Don&apos;t trust us. Verify.</h3>
          <div className="kv"><span className="k">Committed claim root</span><span className="v mono">{short(cfg?.claimRoot)}</span></div>
          <div className="kv"><span className="k">Program</span><span className="v mono">{cfg ? <a href={`https://explorer.solana.com/address/${cfg.programId}${cluster}`} target="_blank">{short(cfg.programId)} ↗</a> : "…"}</span></div>
          <div className="kv"><span className="k">Token</span><span className="v mono">{cfg ? <a href={`https://explorer.solana.com/address/${cfg.mint}${cluster}`} target="_blank">{short(cfg.mint)} ↗</a> : "…"}</span></div>
          <div className="kv"><span className="k">Full allocation</span><span className="v"><a href="/claims.json" target="_blank">public ↗</a></span></div>
          <div className="note">
            The recipient list is committed to the Merkle root above, anchored on-chain
            <b style={{ color: "var(--fg)" }}> before</b> anyone can claim. Pull the public posts,
            apply the published rules, and you get the exact same winners and amounts — the
            on-chain root proves the list wasn&apos;t changed after the fact.
          </div>
          <div className="cta" style={{ justifyContent: "flex-start", marginTop: 14 }}>
            <a className="ghost" href="/verify">See the full proof</a>
          </div>
        </div>
      </div>
    </div>
  );
}

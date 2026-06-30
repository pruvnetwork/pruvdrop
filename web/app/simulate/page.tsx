"use client";
import { useEffect, useState } from "react";
import { runSim, claimRootOf, computeProofHash, randomSeed, type SimCandidate, type SimResult } from "@/lib/sim";

export default function Simulate() {
  const [pool, setPool] = useState<SimCandidate[]>([]);
  const [n, setN] = useState(10);
  const [pot, setPot] = useState(50000);
  const [mode, setMode] = useState<"topn" | "lottery">("lottery");
  const [seed, setSeed] = useState("ansem-2026");
  const [res, setRes] = useState<SimResult | null>(null);
  const [tampered, setTampered] = useState<string | null>(null);
  const [verify, setVerify] = useState<null | boolean>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    fetch("/leaderboard.json").then((r) => r.json())
      .then((rows: any[]) => setPool(rows.map((r) => ({ handle: r.handle, score: r.score }))))
      .catch(() => setPool(Array.from({ length: 40 }, (_, i) => ({ handle: `user_${i + 1}`, score: Math.round((40 - i) * 7.3 * 10) / 10 }))));
  }, []);

  async function run() {
    if (!pool.length) return;
    setBusy(true); setTampered(null); setVerify(null);
    const r = await runSim(pool, { n, pot, mode, seed });
    setRes(r); setBusy(false);
  }

  async function verifyProof() {
    if (!res) return;
    const recomputed = await computeProofHash(res.inputRoot, res.seedHex, res.claimRoot, res.mode, res.n, res.pot);
    setVerify(recomputed === res.proofHash);
  }

  async function tamper() {
    if (!res) return;
    const winners = res.winners.map((w, i) => (i === 0 ? { ...w, amount: w.amount + 1 } : w));
    const newRoot = await claimRootOf(winners);
    setTampered(newRoot);
  }

  const short = (h: string) => `${h.slice(0, 10)}…${h.slice(-8)}`;

  return (
    <div className="page">
      <a className="back" href="/">← back</a>
      <div className="hero" style={{ paddingTop: 18 }}>
        <span className="pill">$ANSEM · simulation</span>
        <h1>See the fairness, live</h1>
        <div className="lead">
          A full provably-fair draw, run <b>entirely in your browser</b> with the same SHA-256
          Merkle scheme as the on-chain program. No backend. Change the inputs, re-run, and try
          to fake a result.
        </div>
      </div>

      <div className="section">
        <div className="panel">
          <div className="simctl">
            <label>Winners <b>{n}</b>
              <input type="range" min={1} max={Math.max(1, Math.min(50, pool.length))} value={n}
                onChange={(e) => setN(Number(e.target.value))} />
            </label>
            <label>Pot ($ANSEM)
              <input type="number" value={pot} onChange={(e) => setPot(Number(e.target.value) || 0)} />
            </label>
            <label>Mode
              <span className="seg">
                <button className={mode === "lottery" ? "on" : ""} onClick={() => setMode("lottery")}>Weighted lottery</button>
                <button className={mode === "topn" ? "on" : ""} onClick={() => setMode("topn")}>Top-N</button>
              </span>
            </label>
            <label>Seed (revealed after commit)
              <span className="seedrow">
                <input value={seed} onChange={(e) => setSeed(e.target.value)} />
                <button className="dice" onClick={() => setSeed(randomSeed())} title="random seed">🎲</button>
              </span>
            </label>
          </div>
          <button className="mt" onClick={run} disabled={busy || !pool.length}>
            {busy ? "Running…" : `Run draw over ${pool.length} candidates`}
          </button>
        </div>

        {res && (
          <>
            <div className="simstep"><span className="sn">1</span><div>
              <div className="st">Commit — input root</div>
              <div className="muted" style={{ fontSize: 12 }}>Merkle root of all {res.poolSize} candidates, committed <b>before</b> the seed.</div>
              <code className="rootline">{res.inputRoot}</code>
            </div></div>

            <div className="simstep"><span className="sn">2</span><div>
              <div className="st">Reveal — draw seed</div>
              <div className="muted" style={{ fontSize: 12 }}>Public randomness (on-chain: a slot hash). Can&apos;t be known at commit time.</div>
              <code className="rootline">{res.seedHex}</code>
            </div></div>

            <div className="simstep"><span className="sn">3</span><div>
              <div className="st">Draw — winners ({res.mode === "lottery" ? "weighted by virality" : "top by virality"})</div>
              <table className="lb" style={{ marginTop: 10 }}>
                <thead><tr><th className="rank">#</th><th>Account</th><th>Wallet</th><th className="num">Score</th><th className="num">$ANSEM</th></tr></thead>
                <tbody>
                  {res.winners.map((w) => (
                    <tr key={w.handle} className={w.rank <= 3 ? `top${w.rank}` : ""}>
                      <td className="rank num">{w.rank}</td>
                      <td className="who">@{w.handle}</td>
                      <td className="mono">{w.wallet.slice(0, 4)}…{w.wallet.slice(-4)}</td>
                      <td className="num">{w.score.toFixed(1)}</td>
                      <td className="num score">{w.amount.toLocaleString()}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div></div>

            <div className="simstep"><span className="sn">4</span><div>
              <div className="st">Claim root — what goes on-chain</div>
              <div className="muted" style={{ fontSize: 12 }}>The winners hash into this root. Anyone recomputes it from the public draw.</div>
              <code className="rootline good">{res.claimRoot}</code>
            </div></div>

            <div className="simstep"><span className="sn">5</span><div>
              <div className="st">Prove <span className="zkbadge">ZK · preview</span></div>
              <div className="muted" style={{ fontSize: 12 }}>
                The public statement a <b>PRUV Halo2 proof</b> binds: that this claim root is the
                correct output of the rules over the committed inputs and seed.
              </div>
              <div style={{ marginTop: 8 }}>
                <div className="kv"><span className="k">input root</span><span className="v mono">{res.inputRoot.slice(0, 22)}…</span></div>
                <div className="kv"><span className="k">seed</span><span className="v mono">{res.seedHex.slice(0, 22)}…</span></div>
                <div className="kv"><span className="k">claim root</span><span className="v mono">{res.claimRoot.slice(0, 22)}…</span></div>
                <div className="kv"><span className="k">rules</span><span className="v mono">{res.mode} · {res.n} winners · {res.pot.toLocaleString()} $ANSEM</span></div>
              </div>
              <div style={{ marginTop: 10 }}>
                <div className="muted" style={{ fontSize: 10.5, textTransform: "uppercase", letterSpacing: ".06em" }}>proof commitment — attested on-chain (proof_hash)</div>
                <code className="rootline">{res.proofHash}</code>
              </div>
              <div style={{ display: "flex", gap: 10, alignItems: "center", marginTop: 10, flexWrap: "wrap" }}>
                <button className="secondary" style={{ width: "auto" }} onClick={verifyProof}>Verify proof</button>
                {verify === true && <span className="good" style={{ fontSize: 13, fontWeight: 600 }}>✓ statement verified — matches the on-chain commitment</span>}
                {verify === false && <span className="bad" style={{ fontSize: 13, fontWeight: 600 }}>✗ mismatch</span>}
              </div>
              <div className="note">
                Today you verify by <b>recomputing</b> (this page just did). <b>Phase 2</b> replaces the
                recompute with a succinct Halo2 proof — verify the whole draw without redoing it.{" "}
                <a href="/verify">how it works →</a>
              </div>
            </div></div>

            <div className="panel" style={{ marginTop: 14, borderColor: tampered ? "var(--bad)" : undefined }}>
              <h3>Try to fake a result</h3>
              <div className="muted" style={{ fontSize: 13, marginBottom: 10 }}>
                Give winner #1 just <b>1 more</b> $ANSEM and recompute the claim root.
              </div>
              <button className="secondary" onClick={tamper}>Tamper winner #1</button>
              {tampered && (
                <div style={{ marginTop: 12 }}>
                  <div className="kv"><span className="k">committed root</span><span className="v mono good">{res.claimRoot.slice(0, 24)}…</span></div>
                  <div className="kv"><span className="k">after tamper</span><span className="v mono bad">{tampered.slice(0, 24)}…</span></div>
                  <div className="note" style={{ color: "var(--bad)" }}>
                    Different root → the pre-committed root no longer matches. Changing <i>any</i> result is detectable. That&apos;s the guarantee.
                  </div>
                </div>
              )}
            </div>
          </>
        )}

        <div className="note center">
          This is the real mechanism, not a mock — it runs the on-chain Merkle scheme in your
          browser. <a href="/verify">How verification works →</a>
        </div>
      </div>
    </div>
  );
}

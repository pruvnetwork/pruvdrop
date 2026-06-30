"use client";
import { useEffect, useState } from "react";

interface SubmitConfig { ticker: string; collectWebhook: string; }

export default function SubmitPage() {
  const [cfg, setCfg] = useState<SubmitConfig>({ ticker: "$TICKER", collectWebhook: "" });
  const [url, setUrl] = useState("");
  const [status, setStatus] = useState<{ msg: string; cls: string }>({ msg: "", cls: "muted" });
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  useEffect(() => {
    fetch("/submit-config.json").then((r) => r.json()).then((c) => setCfg((p) => ({ ...p, ...c }))).catch(() => {});
  }, []);

  async function check() {
    if (!url.trim()) { setStatus({ msg: "Paste your tweet link first.", cls: "warn" }); return; }
    setBusy(true); setStatus({ msg: "Checking…", cls: "muted" });
    let v: any;
    try {
      const r = await fetch(`/api/submit`, {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id: url.trim(), ticker: cfg.ticker }),
      });
      v = await r.json();
    } catch { setStatus({ msg: "Could not reach the validator. Try again.", cls: "bad" }); setBusy(false); return; }

    if (v.ok) {
      const tail = v.persisted
        ? '<br><span class="muted">Entry recorded. Final ranking is by virality.</span>'
        : '<br><span class="muted">Verified. Final ranking is by virality.</span>';
      setStatus({
        msg: `✅ <b>You qualify!</b> @${v.handle} · ${v.likes} likes<br><span class="mono muted">wallet ${v.wallet.slice(0, 6)}…${v.wallet.slice(-4)} verified from your tweet</span>${tail}`,
        cls: "good",
      });
      setDone(true); setBusy(false); return;
    }
    const m: Record<string, string> = {
      no_ticker: `⚠️ Your tweet doesn't mention <b>${cfg.ticker}</b>. Add it and tweet again.`,
      no_wallet: `⚠️ No Solana address found in your tweet. Add your <b>Solana wallet address</b> to the tweet text and resubmit.`,
      not_found: "Couldn't read that tweet. Check the link is public and correct.",
      bad_url: "That doesn't look like a tweet link.",
    };
    setStatus({ msg: m[v.reason] || "Could not validate that tweet.", cls: v.reason === "no_ticker" || v.reason === "no_wallet" ? "warn" : "bad" });
    setBusy(false);
  }

  return (
    <div className="wrap">
      <div className="card">
        <span className="pill">{cfg.ticker}</span>
        <h1>Submit your tweet</h1>
        <div className="sub">Tweet about the campaign, include your Solana address in the tweet, then paste the link. Eligibility is proven on-chain.</div>
        <div className="steps">
          <b>To qualify:</b><br />
          1. Tweet and mention <b>{cfg.ticker}</b><br />
          2. Include your <b>Solana wallet address</b> in the tweet text<br />
          3. Paste the tweet link below
        </div>
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter") check(); }}
          placeholder="https://x.com/you/status/123..."
        />
        <button className="mt" onClick={check} disabled={busy || done}>{done ? "Checked ✓" : "Check my tweet"}</button>
        {status.msg && <div className={`status ${status.cls}`} dangerouslySetInnerHTML={{ __html: status.msg }} />}
      </div>
    </div>
  );
}

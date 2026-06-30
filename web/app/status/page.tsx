"use client";
import { useEffect, useState } from "react";

interface Row { handle: string; posts: number; likes: number; score: number; }

export default function Status() {
  const [all, setAll] = useState<Row[]>([]);
  const [source, setSource] = useState<"submissions" | "farcaster">("farcaster");
  const [q, setQ] = useState("");
  const [res, setRes] = useState<{ rank: number; total: number; row: Row } | "none" | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const live = await fetch("/api/leaderboard").then((r) => r.json());
        if (live?.source === "submissions" && live.rows?.length) { setAll(live.rows); setSource("submissions"); return; }
      } catch {}
      try { const fc = await fetch("/leaderboard.json").then((r) => r.json()); setAll(fc); setSource("farcaster"); } catch {}
    })();
  }, []);

  function check() {
    const h = q.trim().replace(/^@/, "").toLowerCase();
    if (!h) return;
    const idx = all.findIndex((r) => r.handle.toLowerCase() === h);
    if (idx < 0) setRes("none");
    else setRes({ rank: idx + 1, total: all.length, row: all[idx] });
  }

  return (
    <div className="page">
      <a className="back" href="/">← back</a>
      <div className="hero" style={{ paddingTop: 18 }}>
        <span className="pill">$ANSEM · status</span>
        <h1>Check your status</h1>
        <div className="lead">
          See your current standing before the draw. Winners are committed on-chain at the
          deadline - keep posting to climb.
        </div>
      </div>

      <div className="section">
        <div className="panel">
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") check(); }}
            placeholder={source === "submissions" ? "your X handle (e.g. @you)" : "your Farcaster handle"}
          />
          <button className="mt" onClick={check}>Check</button>

          {res === "none" && (
            <div className="status warn">
              You&apos;re not on the board yet. <a href="/submit">Submit your $ANSEM tweet →</a>
            </div>
          )}
          {res && res !== "none" && (
            <div style={{ marginTop: 16 }}>
              <div className="amount">#{res.rank}<span style={{ fontSize: 16, color: "var(--faint)" }}> / {res.total}</span></div>
              <div className="muted" style={{ fontSize: 12, marginBottom: 12 }}>@{res.row.handle}</div>
              <div className="row"><span className="k">Score</span><span className="mono score">{res.row.score.toFixed(1)}</span></div>
              <div className="row"><span className="k">Posts</span><span className="mono">{res.row.posts}</span></div>
              <div className="row"><span className="k">Likes</span><span className="mono">{res.row.likes.toLocaleString()}</span></div>
            </div>
          )}
        </div>
        <div className="note center">
          Final winners are picked deterministically from these public posts and committed
          on-chain - <a href="/verify">verify the fairness →</a>
        </div>
      </div>
    </div>
  );
}

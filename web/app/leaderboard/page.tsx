"use client";
import { useEffect, useState } from "react";

interface Row { handle: string; posts: number; likes: number; score: number; }

export default function Leaderboard() {
  const [rows, setRows] = useState<Row[]>([]);
  const [source, setSource] = useState<"submissions" | "farcaster">("farcaster");
  const [err, setErr] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const live = await fetch("/api/leaderboard").then((r) => r.json());
        if (live?.source === "submissions" && live.rows?.length) {
          setRows(live.rows); setSource("submissions"); return;
        }
      } catch {}
      // fallback: static Farcaster standings
      try {
        const fc = await fetch("/leaderboard.json").then((r) => r.json());
        setRows(fc); setSource("farcaster");
      } catch { setErr(true); }
    })();
  }, []);

  const profile = (h: string) => (source === "submissions" ? `https://x.com/${h}` : `https://warpcast.com/${h}`);

  return (
    <div className="page">
      <a className="back" href="/">← back</a>
      <div className="hero" style={{ paddingTop: 18 }}>
        <span className="pill">$ANSEM · {source === "submissions" ? "X" : "Farcaster"} · live</span>
        <h1>Leaderboard</h1>
        <div className="lead">
          The most viral {source === "submissions" ? "submitted" : "Farcaster"} posts mentioning $ANSEM, ranked.
          Final winners are <b>committed on-chain before the draw</b> — climb by posting, verify the result yourself.
        </div>
      </div>

      <div className="section">
        <div className="panel">
          {err ? (
            <div className="status bad">Could not load standings.</div>
          ) : rows.length === 0 ? (
            <div className="status muted">No entries yet — be the first. <a href="/submit">Submit your tweet →</a></div>
          ) : (
            <table className="lb">
              <thead>
                <tr>
                  <th className="rank">#</th>
                  <th>Account</th>
                  <th className="num">Posts</th>
                  <th className="num">Likes</th>
                  <th className="num">Score</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={r.handle} className={i < 3 ? `top${i + 1}` : ""}>
                    <td className="rank num">{i + 1}</td>
                    <td className="who"><a href={profile(r.handle)} target="_blank">@{r.handle}</a></td>
                    <td className="num">{r.posts}</td>
                    <td className="num">{r.likes.toLocaleString()}</td>
                    <td className="num score">{r.score.toFixed(1)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <div className="note center">
          Standings update as new posts come in. Allocation is deterministic from these
          public posts — <a href="/verify">verify the fairness →</a>
        </div>
      </div>

      <div className="cta" style={{ marginTop: 28 }}>
        <a className="primary" href="/submit">Submit your tweet</a>
        <a className="ghost" href="/claim">Claim</a>
      </div>
    </div>
  );
}

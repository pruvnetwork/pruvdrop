export default function Home() {
  const mint = "9cRCn9rGT8V2imeM2BaKs13yhMEais3ruM3rPvTGpump";
  return (
    <div className="wrap">
      <div className="card">
        <span className="pill">Solana · $ANSEM</span>
        <h1>$ANSEM Airdrop</h1>
        <div className="sub">
          <b>The Black Bull.</b> The most viral $ANSEM posts win $ANSEM. Provably fair —
          the allocation is committed before the draw and is recomputable by anyone.
          No insider list.
        </div>
        <div className="steps">
          <b>How it works</b><br />
          1. Tweet about <b>$ANSEM</b> and include your <b>Solana address</b> in the tweet<br />
          2. Submit your tweet link — instant check that you qualify<br />
          3. Most viral posts win — claim your $ANSEM when winners are allocated
        </div>
        <div className="links">
          <a href="/submit">Submit your tweet</a>
          <a href="/claim">Claim your airdrop</a>
        </div>
        <div className="row" style={{ marginTop: 18 }}>
          <span className="k">Token</span>
          <span className="mono">{mint.slice(0, 6)}…{mint.slice(-4)}</span>
        </div>
      </div>
    </div>
  );
}

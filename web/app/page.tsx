export default function Home() {
  return (
    <div className="wrap">
      <div className="card">
        <span className="pill">Solana · verifiable</span>
        <h1>pruvdrop</h1>
        <div className="sub">
          A provably-fair viral airdrop. Allocation is committed before the
          randomness is known and is recomputable by anyone — no insider list.
        </div>
        <div className="links">
          <a href="/submit">Submit your tweet</a>
          <a href="/claim">Claim your airdrop</a>
        </div>
        <div className="status muted mt">
          Tweet about the campaign with your Solana address to qualify, then claim
          once winners are allocated.
        </div>
      </div>
    </div>
  );
}

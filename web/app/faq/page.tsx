const QA: { q: string; a: React.ReactNode }[] = [
  {
    q: "What is pruvdrop?",
    a: <>A provably-fair $ANSEM airdrop to the people with the most viral posts about $ANSEM. There is no insider list: the recipient list is committed on-chain before the draw and anyone can recompute it.</>,
  },
  {
    q: "How do I qualify?",
    a: <>Post about <b>$ANSEM</b> and include your <b>Solana wallet address</b> in the tweet, then submit the link on the <a href="/submit">Submit</a> page. On Farcaster, having a Solana address verified on your profile is enough.</>,
  },
  {
    q: "How are winners chosen?",
    a: <>By a virality score from public engagement (likes, reposts, replies). It is deterministic — the same public posts and the same published rules always produce the same winners. No operator discretion.</>,
  },
  {
    q: "Is it really fair?",
    a: <>Yes, and you don&apos;t have to trust us. The allocation is committed to an on-chain Merkle root <b>before</b> the draw, and the inputs are public, so anyone can recompute the result and check it matches. See <a href="/verify">Verify</a>.</>,
  },
  {
    q: "Why put my wallet in the tweet?",
    a: <>So the reward is bound to you trustlessly — only you can author your own tweet, so the address inside it is provably yours. No login, no KYC, no insider mapping.</>,
  },
  {
    q: "When is the draw, and how do I claim?",
    a: <>See the live countdown on the <a href="/">home page</a>. After the draw, claiming opens on the <a href="/claim">Claim</a> page — connect the wallet you used. Each wallet can claim once (enforced on-chain).</>,
  },
  {
    q: "What about bots and sybils?",
    a: <>Low-quality accounts are filtered and the scoring methodology is public and challengeable. The goal is to make gaming engagement cost more than the reward. Final winners are re-validated from the public posts at draw time.</>,
  },
  {
    q: "What chain and token?",
    a: <>Solana. The token is $ANSEM; the mint and the distributor program are shown on the <a href="/verify">Verify</a> page.</>,
  },
];

export default function FAQ() {
  return (
    <div className="page">
      <a className="back" href="/">← back</a>
      <div className="hero" style={{ paddingTop: 18 }}>
        <span className="pill">$ANSEM · FAQ</span>
        <h1>How it works</h1>
        <div className="lead">Everything about qualifying, fairness, and claiming.</div>
      </div>

      <div className="section">
        {QA.map((item, i) => (
          <div className="panel" key={i} style={{ marginBottom: 12 }}>
            <h3>{item.q}</h3>
            <div className="muted" style={{ fontSize: 14, lineHeight: 1.6 }}>{item.a}</div>
          </div>
        ))}
      </div>

      <div className="cta" style={{ marginTop: 8 }}>
        <a className="primary" href="/submit">Submit your tweet</a>
        <a className="ghost" href="/verify">Verify fairness</a>
      </div>
    </div>
  );
}

export default function Footer() {
  return (
    <footer className="ftr">
      <div className="ftr-inner">
        <div className="ftr-brand">
          <img src="/logo.svg" alt="" width={20} height={20} />
          <span>pruvdrop</span>
          <span className="ftr-dim">· provably-fair airdrops</span>
        </div>
        <nav className="ftr-nav">
          <a href="/submit">Submit</a>
          <a href="/leaderboard">Leaderboard</a>
          <a href="/status">Status</a>
          <a href="/verify">Verify</a>
          <a href="/faq">FAQ</a>
          <a href="https://github.com/pruvnetwork/pruvdrop" target="_blank">GitHub ↗</a>
        </nav>
      </div>
      <div className="ftr-note">Built with PRUV · committed before the draw · recomputable by anyone · no insider list</div>
    </footer>
  );
}

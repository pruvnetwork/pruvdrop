"use client";
import { usePathname } from "next/navigation";
import dynamic from "next/dynamic";
import ThemeToggle from "./ThemeToggle";

const WalletMultiButton = dynamic(
  () => import("@solana/wallet-adapter-react-ui").then((m) => m.WalletMultiButton),
  { ssr: false }
);

const NAV = [
  { href: "/submit", label: "Submit" },
  { href: "/leaderboard", label: "Leaderboard" },
  { href: "/status", label: "Status" },
  { href: "/simulate", label: "Simulate" },
  { href: "/verify", label: "Verify" },
  { href: "/faq", label: "FAQ" },
  { href: "/claim", label: "Claim" },
];

export default function Header() {
  const path = usePathname();
  return (
    <header className="hdr">
      <a className="hdr-brand" href="/">
        {/* same PRUV brand mark */}
        <img src="/logo.svg" alt="pruvdrop" width={26} height={26} />
        <span className="hdr-name">PRUVDROP</span>
        <span className="hdr-badge">devnet</span>
      </a>
      <nav className="hdr-nav">
        {NAV.map((n) => (
          <a key={n.href} href={n.href} className={path === n.href ? "active" : ""}>{n.label}</a>
        ))}
      </nav>
      <div className="hdr-right">
        <a className="hdr-icon" href="https://github.com/pruvnetwork/pruvdrop" target="_blank" rel="noopener noreferrer" aria-label="GitHub">
          <svg width="17" height="17" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 .5C5.37.5 0 5.78 0 12.29c0 5.2 3.44 9.6 8.21 11.16.6.11.82-.25.82-.56 0-.28-.01-1.02-.02-2-3.34.71-4.04-1.58-4.04-1.58-.55-1.36-1.33-1.73-1.33-1.73-1.09-.73.08-.71.08-.71 1.2.08 1.84 1.21 1.84 1.21 1.07 1.79 2.81 1.27 3.5.97.11-.76.42-1.27.76-1.56-2.67-.3-5.47-1.31-5.47-5.83 0-1.29.47-2.34 1.24-3.17-.13-.3-.54-1.52.12-3.17 0 0 1.01-.32 3.3 1.21.96-.26 1.98-.39 3-.4 1.02.01 2.04.14 3 .4 2.29-1.53 3.3-1.21 3.3-1.21.66 1.65.25 2.87.12 3.17.77.83 1.24 1.88 1.24 3.17 0 4.53-2.81 5.53-5.49 5.82.43.36.81 1.09.81 2.2 0 1.59-.01 2.87-.01 3.26 0 .31.22.68.83.56C20.56 21.88 24 17.49 24 12.29 24 5.78 18.63.5 12 .5z"/></svg>
        </a>
        <a className="hdr-icon" href="https://x.com/pruvfun" target="_blank" rel="noopener noreferrer" aria-label="X">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231 5.45-6.231zm-1.161 17.52h1.833L7.084 4.126H5.117l11.966 15.644z"/></svg>
        </a>
        <ThemeToggle />
        <WalletMultiButton />
      </div>
    </header>
  );
}

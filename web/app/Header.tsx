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
  { href: "/simulate", label: "Simulate" },
  { href: "/verify", label: "Verify" },
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
        <ThemeToggle />
        <WalletMultiButton />
      </div>
    </header>
  );
}

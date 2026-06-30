import type { Metadata } from "next";
import { Inter, JetBrains_Mono } from "next/font/google";
import "./globals.css";
import Header from "./Header";

const inter = Inter({ subsets: ["latin"], variable: "--font-body", display: "swap" });
const mono = JetBrains_Mono({ subsets: ["latin"], variable: "--font-mono", display: "swap" });

const DESC = "Provably-fair $ANSEM airdrop on Solana. Committed before the draw, recomputable by anyone. No insider list.";

export const metadata: Metadata = {
  metadataBase: new URL("https://pruvdrop.vercel.app"),
  title: "pruvdrop — $ANSEM verifiable viral airdrop",
  description: DESC,
  openGraph: {
    title: "$ANSEM Airdrop — pruvdrop",
    description: DESC,
    url: "https://pruvdrop.vercel.app",
    siteName: "pruvdrop",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: "$ANSEM Airdrop — pruvdrop",
    description: DESC,
  },
};

const themeScript = `(function(){try{var t=localStorage.getItem('theme')||'light';document.documentElement.setAttribute('data-theme',t);}catch(e){document.documentElement.setAttribute('data-theme','light');}})();`;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${inter.variable} ${mono.variable}`} suppressHydrationWarning>
      <body>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
        <Header />
        {children}
      </body>
    </html>
  );
}

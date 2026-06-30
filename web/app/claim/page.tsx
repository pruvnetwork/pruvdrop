"use client";
import { useEffect, useState } from "react";
import { Connection, Transaction, PublicKey } from "@solana/web3.js";
import { useWallet } from "@solana/wallet-adapter-react";
import dynamic from "next/dynamic";
import {
  claimIx, createAtaIdempotentIx, ataFor,
  type AirdropConfig, type ClaimsMap, type ClaimEntry,
} from "@/lib/claim";

const WalletMultiButton = dynamic(
  () => import("@solana/wallet-adapter-react-ui").then((m) => m.WalletMultiButton),
  { ssr: false }
);

export default function ClaimPage() {
  const { publicKey, sendTransaction } = useWallet();
  const [cfg, setCfg] = useState<AirdropConfig | null>(null);
  const [claims, setClaims] = useState<ClaimsMap>({});
  const [status, setStatus] = useState<{ msg: string; cls: string }>({ msg: "", cls: "muted" });
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const [c, cl] = await Promise.all([
          fetch("/config.json").then((r) => r.json()),
          fetch("/claims.json").then((r) => r.json()),
        ]);
        setCfg(c); setClaims(cl);
      } catch { setStatus({ msg: "Failed to load campaign data.", cls: "bad" }); }
    })();
  }, []);

  const key = publicKey?.toBase58();
  const myClaim: ClaimEntry | undefined = key ? claims[key] : undefined;

  async function onClaim() {
    if (!cfg || !publicKey || !myClaim) return;
    setBusy(true); setStatus({ msg: "Building transaction…", cls: "muted" });
    try {
      const conn = new Connection(cfg.rpcUrl, "confirmed");
      const mint = new PublicKey(cfg.mint);
      const tx = new Transaction();
      tx.add(createAtaIdempotentIx(publicKey, ataFor(publicKey, mint), publicKey, mint));
      tx.add(claimIx(cfg, myClaim, publicKey));
      tx.feePayer = publicKey;
      tx.recentBlockhash = (await conn.getLatestBlockhash()).blockhash;
      setStatus({ msg: "Approve in your wallet…", cls: "muted" });
      const sig = await sendTransaction(tx, conn);
      setStatus({ msg: "Confirming…", cls: "muted" });
      await conn.confirmTransaction(sig, "confirmed");
      const cluster = cfg.rpcUrl.includes("devnet") ? "?cluster=devnet" : "";
      setStatus({ msg: `Claimed. <a href="https://explorer.solana.com/tx/${sig}${cluster}" target="_blank">View transaction →</a>`, cls: "good" });
      setDone(true);
    } catch (e: any) {
      const m = e?.message ?? String(e);
      setStatus({
        msg: m.includes("already in use") || m.includes("0x0") ? "Already claimed for this wallet." : "Failed: " + m,
        cls: "bad",
      });
    } finally { setBusy(false); }
  }

  return (
    <div className="wrap">
      <div className="card">
        <span className="pill">Claim</span>
        <h1>Claim your airdrop</h1>
        <div className="sub">Connect the wallet that holds your award. Payout is proven on-chain against a committed Merkle root.</div>

        {!publicKey && <div className="mt"><WalletMultiButton /></div>}

        {publicKey && (
          <>
            <div className="pill">{key!.slice(0, 4)}…{key!.slice(-4)}</div>
            {myClaim ? (
              <>
                <div className="amount">{myClaim.amount}</div>
                <div className="muted" style={{ fontSize: 12, marginBottom: 14 }}>claimable (base units)</div>
                <div className="row"><span className="k">Index</span><span className="mono">{myClaim.index}</span></div>
                <div className="row"><span className="k">Proof depth</span><span className="mono">{myClaim.proof.length}</span></div>
                <button className="mt" onClick={onClaim} disabled={busy || done}>{done ? "Claimed" : "Claim"}</button>
              </>
            ) : (
              <div className="status bad" style={{ marginTop: 6 }}>No award found for this wallet.</div>
            )}
          </>
        )}

        {status.msg && <div className={`status ${status.cls}`} dangerouslySetInnerHTML={{ __html: status.msg }} />}
      </div>
    </div>
  );
}

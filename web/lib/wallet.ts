"use client";
import { useCallback, useEffect, useState } from "react";
import { PublicKey, Transaction } from "@solana/web3.js";

type PhantomProvider = {
  isPhantom?: boolean;
  publicKey?: { toString(): string } | null;
  connect: () => Promise<{ publicKey: { toString(): string } }>;
  disconnect: () => Promise<void>;
  signAndSendTransaction: (tx: Transaction) => Promise<{ signature: string }>;
  on?: (event: string, cb: () => void) => void;
};

declare global {
  interface Window { solana?: PhantomProvider }
}

export function usePhantom() {
  const [pubkey, setPubkey] = useState<PublicKey | null>(null);
  const [available, setAvailable] = useState(false);

  useEffect(() => {
    const p = typeof window !== "undefined" ? window.solana : undefined;
    setAvailable(!!p?.isPhantom);
    if (p?.publicKey) setPubkey(new PublicKey(p.publicKey.toString()));
  }, []);

  const connect = useCallback(async () => {
    const p = window.solana;
    if (!p?.isPhantom) throw new Error("Phantom wallet not found");
    const res = await p.connect();
    const pk = new PublicKey(res.publicKey.toString());
    setPubkey(pk);
    return pk;
  }, []);

  const signAndSend = useCallback(async (tx: Transaction) => {
    const p = window.solana;
    if (!p) throw new Error("wallet not connected");
    const { signature } = await p.signAndSendTransaction(tx);
    return signature;
  }, []);

  return { pubkey, available, connect, signAndSend };
}

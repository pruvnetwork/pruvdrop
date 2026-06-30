"use client";
import { useEffect, useState } from "react";

export default function Countdown({ to }: { to: string }) {
  const [now, setNow] = useState<number | null>(null);

  useEffect(() => {
    setNow(Date.now());
    const i = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(i);
  }, []);

  const target = new Date(to).getTime();
  const pad = (n: number) => String(n).padStart(2, "0");

  let body: React.ReactNode = "—";
  if (now !== null) {
    let diff = target - now;
    if (isNaN(target)) body = "TBA";
    else if (diff <= 0) body = "Draw closed";
    else {
      const d = Math.floor(diff / 86400000); diff -= d * 86400000;
      const h = Math.floor(diff / 3600000); diff -= h * 3600000;
      const m = Math.floor(diff / 60000); diff -= m * 60000;
      const s = Math.floor(diff / 1000);
      body = `${d}d ${pad(h)}h ${pad(m)}m ${pad(s)}s`;
    }
  }

  return (
    <div className="countdown">
      <span className="cdlabel">Draw in</span>
      <span className="cdval">{body}</span>
    </div>
  );
}

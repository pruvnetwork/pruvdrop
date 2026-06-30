import { ImageResponse } from "next/og";

export const alt = "pruvdrop — $ANSEM verifiable viral airdrop";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

export default function Image() {
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          padding: "64px 72px",
          background: "#070a14",
          backgroundImage: "linear-gradient(120deg, rgba(37,99,235,0.22), rgba(7,10,20,0) 42%, rgba(124,58,237,0.26))",
          color: "#f8fafc",
          fontFamily: "sans-serif",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <div style={{ display: "flex", fontSize: 32, fontWeight: 800, letterSpacing: -0.5 }}>
            PRUVDROP
          </div>
          <div
            style={{
              display: "flex",
              fontSize: 22,
              color: "#b5bed0",
              border: "1px solid rgba(124,58,237,0.45)",
              padding: "8px 18px",
              borderRadius: 999,
            }}
          >
            Solana · provably fair
          </div>
        </div>

        <div style={{ display: "flex", flexDirection: "column" }}>
          <div style={{ display: "flex", fontSize: 90, fontWeight: 800, letterSpacing: -2, lineHeight: 1.04 }}>
            <span style={{ color: "#a78bfa" }}>$ANSEM</span>
            <span style={{ color: "#f8fafc", marginLeft: 26 }}>Airdrop</span>
          </div>
          <div style={{ display: "flex", fontSize: 42, color: "#b5bed0", marginTop: 20 }}>
            Provably fair. No insider list. Recompute it yourself.
          </div>
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            fontSize: 25,
            color: "#7c8495",
          }}
        >
          <div style={{ display: "flex" }}>Committed on-chain before the draw</div>
          <div style={{ display: "flex", color: "#b5bed0" }}>pruvdrop.vercel.app</div>
        </div>
      </div>
    ),
    { ...size }
  );
}

// POST /api/submit  { id, ticker }  ->  validates server-side and records the entry.
// Storage: Vercel KV (free tier) if configured (KV_REST_API_URL set); otherwise the
// entry is validated but not persisted (persisted:false) so the demo still works.
import { validateTweet } from "@/lib/tweet";

const KEY = "pruvdrop:submissions";

async function store(entry: { id: string; wallet: string; handle: string; likes: number; ts: number }): Promise<boolean> {
  if (!process.env.KV_REST_API_URL) return false;
  try {
    const { kv } = await import("@vercel/kv");
    // hash keyed by tweet id -> automatic dedupe on resubmission
    await kv.hset(KEY, { [entry.id]: JSON.stringify(entry) });
    return true;
  } catch {
    return false;
  }
}

export async function POST(req: Request) {
  let body: any = {};
  try { body = await req.json(); } catch {}
  const ref = String(body.id || "");
  const ticker = String(body.ticker || "");

  const v = await validateTweet(ref, ticker);
  if (!v.ok) return Response.json({ ...v, persisted: false });

  const persisted = await store({
    id: v.id!, wallet: v.wallet!, handle: v.handle || "", likes: v.likes || 0, ts: Date.now(),
  });
  return Response.json({ ...v, persisted });
}

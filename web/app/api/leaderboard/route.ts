// GET /api/leaderboard  ->  live standings from KV submissions (re-fetched engagement).
// Cached (revalidate) so it recomputes at most every couple of minutes.
// If KV is empty/unconfigured, returns { source: "none" } and the page falls back
// to the static leaderboard.json.
import { fetchTweetMeta } from "@/lib/tweet";

export const revalidate = 120;
const KEY = "pruvdrop:submissions";
const CAP = 150;
const CONCURRENCY = 12;

async function getSubmissions(): Promise<any[]> {
  if (!process.env.KV_REST_API_URL) return [];
  try {
    const { kv } = await import("@vercel/kv");
    const map = (await kv.hgetall(KEY)) || {};
    return Object.values(map).map((v: any) => (typeof v === "string" ? JSON.parse(v) : v));
  } catch {
    return [];
  }
}

async function pool<T, R>(items: T[], n: number, fn: (t: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let i = 0;
  async function worker() { while (i < items.length) { const idx = i++; out[idx] = await fn(items[idx]); } }
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, worker));
  return out;
}

export async function GET() {
  const subs = await getSubmissions();
  if (!subs.length) return Response.json({ source: "none", rows: [] });

  const capped = subs.slice(0, CAP);
  const metas = await pool(capped, CONCURRENCY, async (s: any) => {
    const m = await fetchTweetMeta(s.id).catch(() => null);
    return { handle: ((m?.handle || s.handle || "") as string).toLowerCase(), likes: m?.likes ?? s.likes ?? 0 };
  });

  const byHandle: Record<string, { handle: string; posts: number; likes: number }> = {};
  for (const m of metas) {
    if (!m.handle) continue;
    const e = byHandle[m.handle] || (byHandle[m.handle] = { handle: m.handle, posts: 0, likes: 0 });
    e.posts += 1;
    e.likes += m.likes;
  }

  const rows = Object.values(byHandle)
    .map((e) => ({ ...e, score: Number(e.likes.toFixed(1)) }))
    .sort((a, b) => b.likes - a.likes);

  return Response.json({ source: "submissions", rows });
}

// GET /api/submissions?token=<ADMIN_TOKEN>  ->  all collected submissions (operator export).
// Export the tweet ids into app/tweets.json and run `npm run campaign:x`.
const KEY = "pruvdrop:submissions";

export async function GET(req: Request) {
  const { searchParams } = new URL(req.url);
  const token = searchParams.get("token") || "";
  if (!process.env.ADMIN_TOKEN || token !== process.env.ADMIN_TOKEN) {
    return new Response("unauthorized", { status: 401 });
  }
  if (!process.env.KV_REST_API_URL) {
    return Response.json({ configured: false, count: 0, submissions: [] });
  }
  try {
    const { kv } = await import("@vercel/kv");
    const map = (await kv.hgetall(KEY)) || {};
    const submissions = Object.values(map).map((v: any) => (typeof v === "string" ? JSON.parse(v) : v));
    return Response.json({ configured: true, count: submissions.length, submissions });
  } catch (e: any) {
    return Response.json({ configured: true, error: e?.message ?? "kv error", submissions: [] }, { status: 500 });
  }
}

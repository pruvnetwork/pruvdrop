// GET /api/validate?id=<tweetUrlOrId>&ticker=<$TICKER>  ->  Verdict
// Instant feedback for the submit form. UX only — run-campaign-x re-validates at ingest.
import { validateTweet } from "@/lib/tweet";

export async function GET(req: Request) {
  const { searchParams } = new URL(req.url);
  const ref = searchParams.get("id") || "";
  const ticker = searchParams.get("ticker") || "";
  const v = await validateTweet(ref, ticker);
  return Response.json(v);
}

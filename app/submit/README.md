# Tweet-submit form (instant validation)

A static form + one serverless function that gives casters **instant feedback**
on whether their tweet qualifies, and collects valid submissions.

- `submit/index.html` — the form (paste tweet link → instant verdict).
- `api/validate.js` — Vercel serverless function. The syndication endpoint only
  allows CORS from `platform.twitter.com`, so the browser can't read tweets
  directly; this function reads the tweet **server-side** and returns
  `{ ok, reason, handle, likes, wallet }`.

## How it works

1. Caster tweets about the ticker **and includes their Solana address** in the text.
2. They paste the tweet link → the form calls `/api/validate`:
   - ✅ ticker present + valid Solana address → "You qualify" (+ optional collection).
   - ⚠️ `no_ticker` / `no_wallet` → tells them exactly what to fix.
3. Valid entries are POSTed to `collectWebhook` (if configured) for collection.

The form is **UX only** — `run-campaign-x` re-validates every tweet at ingest, so
the form cannot inject bad entries.

## Deploy (Vercel)

Deploy the **`app/` directory** as the project root so the function and pages are served:

```
Root Directory: app
```

- `/submit/`  → the form
- `/api/validate` → the validator
- `/portal/`  → the claim portal

## Collecting submissions

Set `submit/config.json` → `collectWebhook` to any endpoint that accepts a JSON POST
of `{ id, wallet, handle, likes }`. No code needed — use a free option:

- **Formspree** (`https://formspree.io/f/xxx`)
- **Google Apps Script** web app (doPost → append to a Sheet)
- **Discord webhook** (collect in a channel)

Then export the collected tweet ids/urls into `app/tweets.json` and run:

```bash
npm run campaign:x -- ./tweets.json ./campaign-x.json
```

## Config (`submit/config.json`)

```json
{ "ticker": "$PRUV", "collectWebhook": "" }
```

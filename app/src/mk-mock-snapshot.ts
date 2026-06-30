import { writeFileSync, mkdirSync } from "node:fs";
import { DEFAULT_CONFIG, type Candidate, type CampaignConfig } from "./types.js";
import { buildSnapshot } from "./snapshot.js";
const camp: CampaignConfig = { ...DEFAULT_CONFIG, query: "$PRUV", windowStart: 0, windowEnd: 1 };
const cands: Candidate[] = Array.from({length: 12}, (_, i) => ({
  fid: 100+i, username: `caster${i}`, wallet: `Wallet${100+i}`, score: (12-i)*7.5 + (i%3),
  castCount: 1+i%3, totalLikes: (12-i)*40, totalRecasts: (12-i)*8, totalReplies: (12-i)*3, qualityScore: 0.7+i*0.02,
}));
mkdirSync("out", { recursive: true });
writeFileSync("out/snapshot.json", JSON.stringify(buildSnapshot(cands, camp), null, 2));
console.log("wrote out/snapshot.json");

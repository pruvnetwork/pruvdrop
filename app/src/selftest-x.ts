/**
 * Offline self-test for the X ingest helpers (no network).
 */
import { parseTweetId, syndicationToken, handleHash, extractSolanaAddress } from "./ingest-x.js";

function assert(c: boolean, m: string) { if (!c) { console.error("FAIL:", m); process.exit(1); } }

// parseTweetId
assert(parseTweetId("https://x.com/jack/status/1234567890123") === "1234567890123", "parse id from x.com url");
assert(parseTweetId("https://twitter.com/a/status/999888777") === "999888777", "parse id from twitter.com url");
assert(parseTweetId("1234567890") === "1234567890", "parse raw id");
let threw = false; try { parseTweetId("not a tweet"); } catch { threw = true; }
assert(threw, "reject unparseable ref");

// syndicationToken: deterministic + non-empty
const t1 = syndicationToken("1234567890123");
assert(t1.length > 0 && t1 === syndicationToken("1234567890123"), "token deterministic + non-empty");

// handleHash: deterministic, case-insensitive, positive int
assert(handleHash("Ansem") === handleHash("ansem"), "handleHash case-insensitive");
assert(Number.isInteger(handleHash("ansem")) && handleHash("ansem") >= 0, "handleHash positive int");
assert(handleHash("a") !== handleHash("b"), "handleHash distinguishes handles");

// extractSolanaAddress
const real = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
assert(extractSolanaAddress(`gm $PRUV my wallet is ${real} send it`) === real, "extracts a valid Solana address");
assert(extractSolanaAddress(`gm $PRUV no address here just words and 123`) === null, "no false positive on plain text");
assert(extractSolanaAddress("") === null, "empty text -> null");
// picks the valid base58 32-byte one even among noise
assert(extractSolanaAddress(`shorttoolong0000 ${real}`) === real, "ignores non-32-byte candidates");

console.log("PASS — X ingest helpers: id parsing, token, handle hash, Solana-address extraction all OK");
console.log(`  sample token: ${t1.slice(0, 12)}…  handleHash(ansem)=${handleHash("ansem")}`);

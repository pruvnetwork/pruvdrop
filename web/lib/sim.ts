// Browser-side provably-fair allocation simulation.
// Uses Web Crypto SHA-256 and the SAME Merkle leaf/node scheme as the on-chain
// claim program — so the roots it shows are authentic, not mocked. No backend.

const enc = new TextEncoder();

async function sha256(bytes: Uint8Array): Promise<Uint8Array> {
  const d = await crypto.subtle.digest("SHA-256", bytes as unknown as BufferSource);
  return new Uint8Array(d);
}
function concat(...arrs: Uint8Array[]): Uint8Array {
  const len = arrs.reduce((n, a) => n + a.length, 0);
  const out = new Uint8Array(len);
  let o = 0;
  for (const a of arrs) { out.set(a, o); o += a.length; }
  return out;
}
function u64le(n: number): Uint8Array {
  const b = new Uint8Array(8);
  let v = BigInt(Math.max(0, Math.floor(n)));
  for (let i = 0; i < 8; i++) { b[i] = Number(v & 0xffn); v >>= 8n; }
  return b;
}
export function hex(b: Uint8Array): string {
  return Array.from(b).map((x) => x.toString(16).padStart(2, "0")).join("");
}
function cmp(a: Uint8Array, b: Uint8Array): number {
  for (let i = 0; i < a.length; i++) { if (a[i] !== b[i]) return a[i] - b[i]; }
  return 0;
}
const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function b58(bytes: Uint8Array): string {
  const digits = [0];
  for (const byte of bytes) {
    let carry = byte;
    for (let j = 0; j < digits.length; j++) { carry += digits[j] << 8; digits[j] = carry % 58; carry = (carry / 58) | 0; }
    while (carry) { digits.push(carry % 58); carry = (carry / 58) | 0; }
  }
  let str = "";
  for (const byte of bytes) { if (byte === 0) str += "1"; else break; }
  for (let i = digits.length - 1; i >= 0; i--) str += B58[digits[i]];
  return str;
}

// claim leaf = sha256(0x00 || u64LE(index) || wallet[32] || u64LE(amount))  (matches on-chain)
async function claimLeaf(index: number, wallet: Uint8Array, amount: number): Promise<Uint8Array> {
  return sha256(concat(Uint8Array.of(0), u64le(index), wallet, u64le(amount)));
}
// node = sha256(0x01 || min(a,b) || max(a,b))  (matches on-chain)
async function node(a: Uint8Array, b: Uint8Array): Promise<Uint8Array> {
  const [lo, hi] = cmp(a, b) <= 0 ? [a, b] : [b, a];
  return sha256(concat(Uint8Array.of(1), lo, hi));
}
async function merkleRoot(leaves: Uint8Array[]): Promise<Uint8Array> {
  if (leaves.length === 0) return new Uint8Array(32);
  let level = leaves;
  while (level.length > 1) {
    const next: Uint8Array[] = [];
    for (let i = 0; i < level.length; i += 2) {
      if (i + 1 < level.length) next.push(await node(level[i], level[i + 1]));
      else next.push(level[i]); // odd carry up
    }
    level = next;
  }
  return level[0];
}
// input commitment leaf — a hash of (handle, score); any consistent scheme works
async function inputLeaf(handle: string, score: number): Promise<Uint8Array> {
  return sha256(concat(Uint8Array.of(2), enc.encode(handle), u64le(Math.round(score * 100))));
}
// deterministic 32-byte "wallet" from a handle (simulation only)
async function fakeWallet(handle: string): Promise<Uint8Array> {
  return sha256(enc.encode("wallet:" + handle));
}
// seeded draw: sha256(seed || u64LE(k)) -> integer in [0, mod)
async function draw(seed: Uint8Array, k: number, mod: number): Promise<number> {
  const h = await sha256(concat(seed, u64le(k)));
  let x = 0n;
  for (let i = 0; i < 8; i++) x = (x << 8n) | BigInt(h[i]);
  return Number(x % BigInt(mod));
}

export interface SimCandidate { handle: string; score: number; }
export interface Winner { rank: number; handle: string; wallet: string; walletBytes: Uint8Array; amount: number; score: number; }
export interface SimResult {
  inputRoot: string; seedHex: string; claimRoot: string;
  winners: Winner[]; poolSize: number; mode: string;
}

export interface SimOpts { n: number; pot: number; mode: "topn" | "lottery"; seed: string }

export async function runSim(pool: SimCandidate[], opts: SimOpts): Promise<SimResult> {
  const cands = pool.filter((c) => c.score > 0);
  const n = Math.max(1, Math.min(opts.n, cands.length));
  const seed = await sha256(enc.encode(opts.seed || "seed"));

  // 1) commit: inputRoot over the full candidate set (before the seed)
  const inLeaves = await Promise.all(cands.map((c) => inputLeaf(c.handle, c.score)));
  const inputRoot = hex(await merkleRoot(inLeaves));

  // 2) select winners
  let selected: SimCandidate[];
  if (opts.mode === "topn") {
    selected = [...cands].sort((a, b) => b.score - a.score || a.handle.localeCompare(b.handle)).slice(0, n);
  } else {
    // weighted lottery: cumulative score buckets, seed-seeded draws, no repeats
    const cum: number[] = []; let acc = 0;
    for (const c of cands) { acc += c.score; cum.push(acc); }
    const total = acc;
    const picked = new Set<number>(); const order: number[] = [];
    let k = 0;
    while (order.length < n && k < n * 50) {
      const r = await draw(seed, k++, Math.max(1, Math.floor(total * 100)));
      const target = r / 100;
      let idx = cum.findIndex((c) => target < c);
      if (idx < 0) idx = cands.length - 1;
      if (!picked.has(idx)) { picked.add(idx); order.push(idx); }
    }
    selected = order.map((i) => cands[i]);
  }

  // 3) amounts — proportional to score
  const sumScore = selected.reduce((s, c) => s + c.score, 0) || 1;
  const winners: Winner[] = [];
  let dist = 0;
  for (let i = 0; i < selected.length; i++) {
    const amt = Math.floor((opts.pot * selected[i].score) / sumScore);
    dist += amt;
    const wb = await fakeWallet(selected[i].handle);
    winners.push({ rank: i + 1, handle: selected[i].handle, wallet: b58(wb), walletBytes: wb, amount: amt, score: selected[i].score });
  }
  if (winners[0]) winners[0].amount += opts.pot - dist; // remainder to #1

  // 4) claimRoot over winner leaves (authentic on-chain scheme)
  const claimLeaves = await Promise.all(winners.map((w, i) => claimLeaf(i, w.walletBytes, w.amount)));
  const claimRoot = hex(await merkleRoot(claimLeaves));

  return { inputRoot, seedHex: hex(seed), claimRoot, winners, poolSize: cands.length, mode: opts.mode };
}

// recompute claimRoot after tampering one winner's amount (for the tamper demo)
export async function claimRootOf(winners: Winner[]): Promise<string> {
  const leaves = await Promise.all(winners.map((w, i) => claimLeaf(i, w.walletBytes, w.amount)));
  return hex(await merkleRoot(leaves));
}

export function randomSeed(): string {
  const b = new Uint8Array(8);
  crypto.getRandomValues(b);
  return hex(b);
}

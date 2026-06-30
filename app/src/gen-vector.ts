import { leafHash, nodeHash, base58Decode } from "./claim-tree.js";
const W0 = "11111111111111111111111111111112";
const W1 = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
console.log("pk0_len", base58Decode(W0).length, "pk1_len", base58Decode(W1).length);
const l0 = leafHash(0, W0, 1000n);
const l1 = leafHash(1, W1, 2500n);
console.log("leaf0", l0.toString("hex"));
console.log("leaf1", l1.toString("hex"));
console.log("root2", nodeHash(l0, l1).toString("hex"));

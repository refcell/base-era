# History raw-block import acceptance

Run `python3 etc/history-devnet/import.py --run-dir "$RUN"`. Optional `--worker-bin`,
`--reference-bin`, `--manifest`, and `--output` arguments override artifact/output defaults. The
runner discovers the live source RPC and L2 genesis from the required run directory. It only
initializes fresh databases under its output directory and never opens or modifies source data.

The test captures and commits the SHA-256 digests of concatenated raw RLP blocks 1 through 22. It
runs full-state imports (without `--no-state`), starts each imported database with HTTP and
`--rpc.eth-proof-window 128`, and compares the canonical head, hash, state root, receipts root, and
a historical state query. Separate fresh databases import blocks 1..18 plus a bad block 19 state
root and blocks 1..19 plus a bad block 20 receipts root using `--fail-on-invalid-block`.

## Migrated result (2026-09-18)

Both independently built artifacts exposed `import`; all [six checks
passed](../etc/history-devnet/evidence/final/import.json).

| Check | History host | Reference Base/reth |
|---|---:|---:|
| Canonical import 1..22 | PASS, exit 0, 49.409 s (0.445 blocks/s) | PASS, exit 0, 1.356 s (16.224 blocks/s) |
| Bad pre-Isthmus block 19 state root | PASS, exit 101, 54.186 s | PASS, exit 101, 1.408 s |
| Bad post-Isthmus block 20 receipts root | PASS, exit 101, 53.969 s | PASS, exit 101, 1.132 s |

Canonical parity was exact at head 22: hash
`0xc53bcde7fb41a7a15f16efcb1f0323fa2ed2405b3b20352ff2ba4b3fde20ad25`, state root
`0xa090e2e96c5fe68e146379a9ec4670cc423dd2d587c15f19e141a5270bab3086`, and receipts root
`0x1d422bc42a4b79a63117e0de09df25fadb47778bfffa4b68067c88d2e96ed3f5`. Both malformed
imports remained transactionally at genesis (head 0), proving neither malformed block nor its
valid input prefix was committed.

RLP SHA-256 commitments are `89b750cd96cd5569a5b4d45461e17df3d61a5875d0378827bd2a4ecef7fe6ec6`
(canonical), `ffd5aa527a6acd6cb5a839e710bff237d0af066d8c441fe24f7a5c5d93a95626`
(bad block 19), and `db63f4310614cab2f56aacf5459f32c6e120c02dc9153e4d7e8c7b1762a048c6`
(bad block 20). The reference retains original execution code, with two CLI exposure edits
recorded in `etc/history-devnet/reference-cli.patch`. Its workspace and lockfile are pruned without
introducing new package versions. This adaptation is committed in `historical/reference`.

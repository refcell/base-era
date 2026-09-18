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

## Original source-spike result (2026-09-18)

The pre-migration source spike reported that both real artifacts exposed `import` and passed.
The table and commitments below are historical context, not results from this checkout. Fresh
migrated evidence is pending at `etc/history-devnet/evidence/final/import.json`.

| Check | History host | Reference Base/reth |
|---|---:|---:|
| Canonical import 1..22 | PASS, exit 0, 49.592 s (0.444 blocks/s) | PASS, exit 0, 1.302 s (16.897 blocks/s) |
| Bad pre-Isthmus block 19 state root | PASS, exit 101, 53.751 s | PASS, exit 101, 1.386 s |
| Bad post-Isthmus block 20 receipts root | PASS, exit 101, 53.836 s | PASS, exit 101, 1.194 s |

Canonical parity was exact at head 22: hash
`0xa9e86115158531889e66f5cdba4ed142a8b3e24bce420afbc281b87a67965f2a`, state root
`0x4d9ded9b8cf17a4bbde69bcf373f94371afa86bb378886121423a72f738118a4`, and receipts root
`0x1d422bc42a4b79a63117e0de09df25fadb47778bfffa4b68067c88d2e96ed3f5`. Both malformed
imports remained transactionally at genesis (head 0), proving neither malformed block nor its
valid input prefix was committed.

RLP SHA-256 commitments are `9484736956156c1c051f366b00d370633b359d170d8df7983b7e2fa7b33a0de4`
(canonical), `28a0e685c0abeaff3942e50acc9aa77eb8936689dcf775ffc3729455d3a64ca6`
(bad block 19), and `6514dfff58ed39d4ec7168d7cf0019e7df4b56767bc5fd5c1fd5cbc8a4079e3f`
(bad block 20). In the source spike, the reference artifact differed from original Base/reth only by
the two CLI exposure edits recorded in `etc/history-devnet/reference-cli.patch`; the migrated
reference contains that adaptation in committed `historical/reference` source.

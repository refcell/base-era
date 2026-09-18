# Frozen dependency sources

`frozen-sources.json` records provenance and byte-level integrity for the three
independent source roots. Digests can be reproduced with:

```sh
(cd PATH && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum)
```

The worker's historical Base copy can be checked through the stable interface
`etc/history-worker/scripts/verify-base.sh`. Sources are committed build inputs;
setup must not clone an upstream tree or apply `reth-history.patch`. The patch is
retained only as an auditable record of the original reth integration delta.

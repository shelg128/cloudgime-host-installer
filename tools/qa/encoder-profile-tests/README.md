# Encoder stability regression suite — permanent Universal Release

This suite links the real `DisplayPrepareHelper` source and tests the actual selector,
cache validation and log reader through reflection. It is not a reimplementation of
encoder selection and does not require a test-framework package.

Run:

```
dotnet run --project tools/qa/encoder-profile-tests/EncoderProfileTests.csproj -c Release -- RESULT.json
```

With no output argument, the test writes its result in a new temp fixture directory.
The 42 tests include the original31 recovered/error/cache cases and11 cross-GPU and
runtime-contract cases. NVIDIA, Intel QuickSync, AMD AMF, unknown GPU, missing hardware,
valid software fallback and manually-disabled runtime selection are covered. These are
selection-logic tests, not claims of physical hardware testing on every vendor.

The original bug and before/after physical-PC02-probe evidence were recorded on
`b544a87` in the isolated research worktree. The fix is now promoted into this Universal
Release worktree. See `PERMANENT-STABILITY.md` for the current installed helper hash,
release gate, preservation boundaries and remaining acceptance work.

The whole helper build must run through `tools/portable/test-encoder-release.ps1`, or
the portable bundle builder's mandatory equivalent. The executable's read-only
`--encoder-runtime-contract` output and the `.encoder-contract.json` receipt are checked
against its exact binary/source hashes. The Universal finalizer and every production
package entry path require the receipt. The new bootstrap checks it before installation.
A synthetic fixture-only package is never installable.

Do not force global NVENC, a particular resolution, or H.264 Baseline merely to pass.
Fresh real encoder probes remain authoritative. An unresolved hardware failure is not
hidden by UI readiness, unrelated codecs, or a stale successful log. Physical video
startup, live latency, and freeze<1% remain separate acceptance gates.

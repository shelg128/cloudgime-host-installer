# Universal Node status HTTPS compatibility

Physical PC-02 recovery uncovered repeated Node status transport failures while the same credential/binding was accepted through system curl (three read-only requests HTTP200). Existing heartbeat already used curl fallback, but Node status did not. After the90s in-memory authorization lease expired the supervisor correctly stopped runtime. This prevented candidate deployment and streaming despite intermittent READY heartbeats.

The status path now uses bounded system curl ONLY if the primary reqwest send fails. HTTP401/403/409 or successful-but-invalid JSON are not retried as success. Both transports enter identical activation-state handling, exact returned binding verification, post-network credential/manifest recheck, and90s lease rules. This is control-plane HTTPS transport compatibility, NOT Direct/Relay media fallback.

Safeguards: system32 executable, default curl config disabled, HTTPS only, no redirect, connect3s/transfer7s/wall9s bound, output64KiB bound, credentials JSON only on stdin, no temp body or argv secrets, stderr never surfaced. No certificate validation bypass and no credential rotation/setup token.

64 supervisor tests passed including6new transport parser/contract tests and existing binding/revocation/cache/DPAPI tests. Release built. Temporary PC-02 runtime-agent deployment used exact baseline hash backup and health-triggered rollback. Initial service-stop attempt did not swap binary; subsequent attempt waited for confirmedSTOPPED before replacing. Runtime recovered and fresh verified activation persisted beyond the90s lease.

Candidate pair deployment before this change timed out while the activation gate repeatedly stopped processes. Actual baseline media hashes were independently confirmed afterward; no rental was created. Do not erase those failure logs or treat a timeout as proof no change occurred.

Not proof of all earlier Sentinel exits or streaming stalls: test-specific high-load timestamp acceptance remains separate. The current machine's provisioning and stream media binaries remain baseline; only supervisor/runtime-agent is this pilot. Backup and result are in workspace `06 TEST/20261001-clock-1915/runtime-deployment.json`.

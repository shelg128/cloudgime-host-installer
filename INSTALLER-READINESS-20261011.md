# Upgrade file-only qualification — 11 October 2026

`HostControlBootstrap.PolicyTests/MatchedUpgradeTests.cs` uses the production `UniversalFileTransaction` on a temporary Windows directory, without services, registry, actual enrollment, DPAPI unprotect or user profiles.339additional assertions pass:9failure cut points across8simulated matched runtime/policy components, content+ACL preservation of identity/credential/profile fixtures, durable journal, retained backup, explicit/repeated rollback, duplicate target and missing source.

This strengthens local file transaction evidence. It does NOT implement or qualify a full Host+Sentinel NSIS transaction, process-crash/power-loss recovery, real licensing or profile-service preservation. Do not weaken the existing managed-node installer guards.

A current matched Sentinel-only installer was built and its138embedded files verified (helper0c0a7736, ASAR133d98a4, shell d89d7a93). Full Universal packaging is not complete: local legacy candidate contains streamerEFF86232 rather than PC02's AF431737, and local shared build outputs were overwritten by later experiments. Do not silently ship a legacy or unqualified experimental streamer. Obtain the qualified binary via authorized PC02 access and record provenance, or separately rebuild/qualify canonical source.

SSH to PC02 was reachable but authentication unavailable; old project key missing. No live services/files, maintenance, pilot, rental/billing/profile, or StarDesk changed in this session. Source runtime is unchanged. Further live qualification requires exact installed baseline, maintenance/idle, full protected backup, independent rollback and fresh backend/Sentinel health checks.

Latest workspace report: `05 LAPORAN/20261011-INSTALLER-READINESS-PARTIAL-SSH-BLOCKED.md`.

# Managed stopped-node upgrade — internal candidate, 11 October 2026

This lane is separate from the ordinary Universal installer. The ordinary managed-state guard remains enabled; there is no flag to bypass it.

## Supported initial scope

- Already provisioned node, Sentinel stopped by the operator, SCM Stopped/Disabled, operator journal STOPPED, pilot disabled and no pending reset.
- Backend device-authenticated read-only snapshot must confirm MAINTENANCE, no owner/claim, no active rental, no cleanup hold. Responses bind package digest + transaction UUID and expire in10seconds.
- Existing live Host/media binaries are checked unchanged. The coordinator does NOT restart Host or StarDesk. Updates that change live Host runtime are rejected.
- Installer stages a complete152-file package under administrator/SYSTEM-only `ProgramData/CloudGime/ManagedUpgradeRecovery/<UUID>`. A pinned manifest prevents stale/mixed payloads. No node credentials are embedded in the package.
- Existing credential/identity, operator stop, registry/service state and managed rental profile file hashes/ACLs are preserved. No license claim or automatic resume is performed.

## Transaction and recovery

`ManagedUpgradeTransaction` records old hashes, ACLs, staged input and backup intent before target replacement, then flushes its journal. A new process can reopen the journal for rollback. Unknown current contents or a corrupt backup are rejected instead of overwritten. New files are quarantined in the recovery directory on rollback.

`ManagedStoppedUpgrade` holds both the machine upgrade lock and EmergencyKill operation lock. It transforms broker/shell policy hashes, the existing STOPPED operator plan, its worker copy and shell configuration journal together, keeping the operator operation ID and state unchanged. Original bytes remain in backups. Live coverage so far uses unchanged helper/shell; changed-helper migration is fixture-tested, not yet physically qualified.

A SYSTEM watchdog runs an independent recovery executable. The real Windows sharing-violation test exercises catch-driven automatic rollback. The12-minute watchdog deadline/process-crash recovery on PC02 is NOT qualified by that test; process exit recovery is separately tested on local temporary fixtures.

CLI, elevated operator only:

```
cloudgime-host-bootstrap.exe managed-upgrade-stage UUID PACKAGE_DIRECTORY MANIFEST_SHA256 --silent
engine.exe managed-upgrade prepare UUID PRIVATE_PACKAGE_DIRECTORY --silent
engine.exe managed-upgrade apply UUID --silent
recovery.exe managed-upgrade rollback UUID --silent
```

The managed NSIS wrapper generates the UUID, stages privately, prepares and applies. It does not call the ordinary Sentinel installer or erase policies. The package is an internal candidate, unsigned; publisher authenticity for public distribution remains a separate signing gate.

## Qualification and boundaries

Read workspace `05 LAPORAN/20261011-MANAGED-UPGRADE-ROLLBACK-PC02.md` for exact current installer hashes, live operations and final status. Do not call one successful metadata/bootstrap update a complete fleet upgrade qualification. Still open: new helper/broker/shell version migration physically tested, changing live Host runtime, active-rental interruption, deadline-watchdog/power loss, new-logon resume, universal clean-install and general release signing.

PC02 must remain at the operator desktop with Sentinel STOPPED unless the owner explicitly asks to resume. Do not reuse an old EmergencyKill operation ID or resume as an installer side effect.

# Universal Node installer v1.2 candidate

This branch replaces machine-specific/manual deployment with a fail-closed Universal Node package pipeline.

## Lifecycle

1. Build Host, Sentinel and Keeper Tunnel from reviewed source revisions.
2. Run `tools/portable/finalize-universal-bundle.ps1` to make a new neutral copy, remove all enrollment/pairing state, add the pinned Sentinel setup, seal every payload file with SHA-256, and validate the contract.
3. Run `tools/portable/build-universal-host-installer.ps1` to build the separate Universal NSIS installer. It never reads the currently installed Host.
4. Install normally to upgrade while preserving Node identity and DPAPI credentials. `/FRESH` is accepted only when no Node identity exists.
5. Provision the Node with one activation license using Host Control / `host-installer provision`.
6. From the pinned local shared desktop, run the installed bootstrap command `finalize-universal-node`. This pins the console SID, pairs Sunshine locally, installs/starts Sentinel Node Broker, and validates required files. Add `--enable-autologon` only after operator approval; the password is validated locally and stored as an LSA private secret, never as `DefaultPassword` or a command-line argument.

## Security boundaries

- Package validation occurs before existing processes, files, drivers, services or registry settings are changed.
- Reparse points, duplicate JSON keys, unsafe paths, unlisted files, hash/size mismatches, embedded credentials, pairing state and enrollment state are rejected.
- Reinstall/repair cannot overwrite an existing Node identity. Decommission is a separate control-plane operation.
- Universal install does not weaken Defender or blank-password policy and does not configure autologon.
- Driver catalog Authenticode is verified and only its signer is added to TrustedPublisher; it is never promoted to Root.
- Bundle SHA-256 proves integrity, not publisher identity. Production release still requires Authenticode signing.

## Verified locally

- Bootstrap and packaging projects compile with zero warnings.
- Pure install/autologon rollback tests pass.
- 30 bundle contract assertions pass, termasuk penolakan fixture/unclassified payload sebagai installer.
- 22 Rust provisioning/transport tests pass.
- PowerShell 5.1 parser accepts pairing, finalizer and builder scripts.
- Fixture inert berhasil melewati sanitize -> seal -> validate -> NSIS, tetapi diberi manifest `fixtureOnly`, nama `CloudgimeHostPackageFixture-NOT-INSTALLABLE.exe`, dan ditolak oleh preflight instalasi.

## Gates before release

Do not mark production-ready until a real source-built package (bukan fixture) melewati disposable clean Windows host: clean install, activation, finalizer, reboot recovery, identity-preserving upgrade, rollback on corrupted payload, uninstallation, native Windows/Android clients and Authenticode verification.

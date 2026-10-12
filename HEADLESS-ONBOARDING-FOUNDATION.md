# Headless onboarding foundation — NOT an installable release

## Scope implemented in this checkpoint

The goal approved by the owner is one installer + one Node license + automatic standard rental-account setup + a confirmed restart + a verified Sentinel gate, without a permanent Host Control GUI. This checkpoint implements **only the foundation**, not the complete flow.

- `HeadlessBundleContract`: bounded exact manifest, path/reparse/inventory/hash checks, no Host Control/open-host-control/desktop shortcuts in the app payload. The existing neutral Universal/PE/encoder contract is still enforced, and driver MSI files remain permitted inside the bundle. Integrity is not publisher authenticity.
- `HeadlessSetupCoordinator`: write-ahead pending step, re-observation after a lost response, exact operation/package/operator binding, explicit legal transitions, fixed error diagnostics. Current stages are FILES_STAGED → NODE_PROVISIONED → OPERATOR_PINNED → ACCOUNTS_PREPARED. Tests use injected OS effects; they do not claim licensing or account creation has run live.
- `HeadlessSetupJournal`: private validated directory, exclusive process lock, bounded exact JSON, CAS/readback, atomic durable writes, corrupt/duplicate/unknown-state rejection. Real fresh-process tests ran in disposable administrator-owned ProgramData fixtures; those fixtures were removed. User temp initially failed parent ACL validation; the guard was preserved, not weakened.
- `HeadlessSetupProcess`: secret through bounded stdin only; no license arguments, no raw child diagnostics, cancellation of timeouts/output floods, inherited provisioning/profiler hooks removed, child handles and pipe tasks observed. Test licenses were inert fixtures, not real Node credentials.
- `WindowsHeadlessSetupPlatform`: **unwired draft**; constructor is private and there is no factory. It is not a reviewed production installed-layout adapter. Hash-pinned execution, full device identity/response validation, interrupted-provision recovery and cross-stage rollback are still required.
- `HeadlessSetupCommands`: only `validate-headless-package <root> --silent` and `headless-setup-plan <root> --silent` execute. Both are read-only and explicitly return installationEnabled=false/readyForRental=false. `headless-setup-install` and `headless-setup-activate` reject with exit53. The existing legacy installer commands remain unchanged; do not treat the diagnostic bootstrap as a ready installer.
- Sentinel source adds CLI-only `--headless-rental-account-status` with admin/private DPAPI checks and secret-free output. Account identity/group/profile/disabled-state validation uses shared pure policy tests. The helper source was compiled/tested locally, NOT installed or repackaged into the existing Sentinel installer.

## Diagnostic payload

Workspace: `02 HASIL BUILD DAN INSTALLER/HEADLESS FOUNDATION 20261012/package-final`.
282 inventoried files; Host app portion contains only `cloudgime-host-bootstrap.exe` (no Tauri GUI or opener). The neutral bundle and inner Sentinel are from qualified release1.5.3. **That inner Sentinel still contains old helper ee3fac59, not the newly added account-status command.** Consequently the folder is deliberately validation-only, not a matched automatic onboarding package.

Verified actual process results: validator/plan exit0; install/activate exit53; forged ready flag, extra secret field and hash mismatch rejected53. Source and runtime of all installed nodes remain unchanged. No Node license was claimed, no real Windows account created, no service/autologon modified and no restart performed in this batch.

## Tests at checkpoint

50 coordinator assertions;23 child-process assertions;13 durable-journal assertions;24 package assertions;13 account-observation policy assertions;622 Launcher tests and TypeScript noEmit pass. Existing Host/coordinator rollback regressions and helper/reset suites also pass. These are NOT a fresh install, auto-logon, first-boot, headless media or runtime availability qualification.

## Remaining implementation — do not conceal these gaps

1. A resumable fresh-install transaction bound to known output roots and immutable executable hashes. Preserve operator account, RDP/SSH, Windows policy, identity and any unrelated data. Existing-node migration must remain a separate guarded transaction.
2. One-time wizard and protected resume state: license never on argv/log, no reissuing identity on retry; integrate existing Rust provision_once safely, including bounded transport, no redirects to arbitrary control plane, validation of all assigned credentials/bindings and partial multi-file writes.
3. First-boot shell ordering: legacy InteractiveRentalMode first confirms an authenticated gate; managed shell setup currently requires that CONFIRMED state. Establish an owned locked pre-boot shell and rollback/confirmation sequence **without allowing an Explorer-only rental desktop and without forging CONFIRMED**. Do not enable account/autologon until this is designed and tested.
4. Transactional activation/autologon with LSA secret, journal and owned rollback. Existing operator/remote recovery access must stay usable. Post-boot verify real rental token, launcher attestation, shell, Host/media and backend readiness. Do not treat SCM Running as rentable.
5. Runtime background completion and backend-approved availability; explicit maintenance/EmergencyKill still win. Account switching/Return to Rental and offline maintenance/repair/uninstall replacements need implementation, not just removing GUI names.
6. Rebuild a matched helper/ASAR/marker/broker package containing any new status/setup commands, no ABI/hash mismatch. Do not put this candidate into 1.5.3 artifacts which are already pinned.
7. PC03 clean-install only after the real first-boot lane is ready, user approves the pending reboot and supplies/authorizes a separate Node license. Never reuse PC02 identity or claim this is blocked solely on license while implementation remains unfinished.

No source merge/push/public installer promotion is implied by this checkpoint. The qualification report is `05 LAPORAN/20261012-HEADLESS-FOUNDATION-PARTIAL.md`.

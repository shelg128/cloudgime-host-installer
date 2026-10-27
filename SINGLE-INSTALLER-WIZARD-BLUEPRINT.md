# CloudGime Universal Node — Single Installer Wizard & Auto-Provisioning Blueprint

**Status:** Product acceptance contract / implementation target  
**Audience:** Product owner, installer, Host, Sentinel, control-plane, QA, and release engineering  
**Scope:** Operator experience for a new CloudGime Universal Node installation

---

## 1. Product goal

A new node must be deployable through **one installer wizard (`Setup.exe`)**.

From an operator perspective, the complete normal flow is only:

1. Run `Setup.exe` as Windows Administrator.
2. Select the installation directory.
3. Enter a CloudGime setup token / license supplied by CloudRental.
4. Click **Activate / Install**.
5. Wait until the wizard reports that the node is **Ready for Rental**.

The operator must not need to separately open CloudGime Host Control, run a terminal command, paste the same token again, manually start Host/Sentinel, configure tunnelling, or interpret internal service errors.

---

## 2. Acceptance criteria

The implementation is accepted only when all criteria below are met on a supported clean Windows machine.

### AC-1 — One installer entry point

- The distributed artifact is one signed `Setup.exe`.
- The normal operator path begins and ends inside this wizard.
- Host Control may remain installed for diagnostics and advanced support, but it is **not required** during a normal installation.

### AC-2 — Directory selection at the start

- Before provisioning begins, the wizard shows a destination-directory page.
- The operator can choose an installation directory.
- The wizard validates that the destination is writable, has sufficient free space, is not an unsafe system directory, and does not conflict with an existing managed node.
- The selected directory is used consistently for the bundle/runtime payload that may be installed outside protected Windows locations.
- Security-sensitive state, service executables, and DPAPI-protected credentials may still use their required protected locations; the wizard must explain this briefly if needed.

### AC-3 — Inline setup token / license activation

- The wizard contains a setup-token/license input page; it is not delegated to a separate Host Control application.
- The operator can submit the token from that page.
- The token is passed through a secret-safe channel:
  - never shown in logs, result JSON, telemetry, command-line arguments, or process listings;
  - cleared from UI memory when no longer needed;
  - never persisted in plaintext.
- Token validation and activation are performed against the CloudRental control plane.
- Invalid, expired, revoked, already-used, wrong-kind, or network-failed token states are shown inline in operator language, with a retry option where safe.
- A failed activation must not leave a partially claimable or falsely ready node.

### AC-4 — Silent component provisioning after valid activation

After valid activation, the installer automatically performs the complete managed provisioning sequence without requiring the operator to launch another app:

1. stage and integrity-verify the Universal bundle;
2. bind the node identity, setup token, PC/node record, and activation record at the control plane;
3. store runtime credentials only through the designated protected-store/DPAPI mechanism;
4. install and configure the Host runtime;
5. install and configure Sentinel and its required helper/runtime components;
6. install/configure Keeper tunnel and managed routing prerequisites;
7. register and start required Windows services/tasks;
8. configure service recovery/watchdog behavior;
9. run required host preflight checks, including selected encoder/capture/runtime compatibility;
10. create only the approved firewall rules; no router port-forward, UPnP, DMZ, or manual router access is required;
11. start Host, Sentinel, and Keeper components in the required dependency order.

The wizard may show progress, but must not ask the operator to perform technical substeps during a normal successful path.

### AC-5 — Automatic first boot and readiness convergence

- If a restart is technically required, the wizard clearly explains it and can request/recommend it from the wizard.
- After restart, a durable first-boot continuation resumes automatically under the correct protected account/service context.
- The operator is not required to open Host Control, paste a token again, or manually start services after restart.
- The system must converge only when backend-observable health requirements are satisfied.
- `ARMED`, `PROVISIONED_WAITING_HEARTBEATS`, or merely “installer completed” are **not** acceptance success states.

### AC-6 — Final success definition: Ready for Rental

The wizard may report installation success only after the backend reports the node as ready for rental.

At minimum, the final success gate requires:

- node identity is bound to the intended control-plane record;
- activation record and runtime credential binding are valid;
- Host service is running and reports healthy/ready;
- Sentinel is installed, running, and reports healthy;
- Keeper tunnel is authenticated and routable by the backend;
- required host capability/preflight checks pass;
- no pending cleanup, stale claim, maintenance conflict, or blocked provisioning state prevents rental;
- backend exposes the intended final state as `READY` / `rentalReady: true`.

The wizard final page must state a clear result, for example:

> **CloudGime Node Ready for Rental**  
> Host, Sentinel, and managed tunnel are running and verified by CloudRental.

It must not state success before this gate passes.

### AC-7 — Failure, recovery, and rollback behavior

- A failure page identifies the failed phase in operator-friendly language and preserves a support-safe diagnostic identifier.
- Secret values must never be shown.
- Retry must be idempotent: it resumes the same installation operation/binding where safe and must not silently consume a new license or create duplicate activation records.
- Rollback must not damage unrelated software or erase auditable control-plane history.
- Incomplete installation must remain unavailable for rental.
- Recovery of runtime credentials must have a headless, authenticated, protected-store path. A normal recovery must not require opening a separate Host Control GUI or entering a second Host Control password.

### AC-8 — Normal runtime behavior after setup

- The Host does not need to be manually opened to stream.
- Host, Sentinel, and Keeper are managed as durable services/tasks according to their component requirements.
- Service crash/restart behavior is configured automatically.
- Streaming route policy remains explicit:
  - renter selects **Direct P2P** or **VPS Relay**;
  - no automatic route fallback or silent route switching;
  - Direct P2P uses STUN best effort and does not require router configuration;
  - Relay is only used when explicitly selected.

---

## 3. Operator-facing wizard flow

```text
Welcome
  ↓
Choose installation folder
  ↓
Preflight: permissions / disk / existing managed-node conflict
  ↓
Enter setup token or license
  ↓
Validate token inline
  ↓
Activate and install automatically
  ├─ Stage & verify bundle
  ├─ Provision node identity and protected credentials
  ├─ Install Host + Sentinel + Keeper
  ├─ Configure services, recovery, firewall, and preflight
  ├─ Start components
  └─ Wait for backend READY / rentalReady:true
  ↓
Success: “Node Ready for Rental”
```

If reboot is required:

```text
Install/activation reaches restart boundary
  ↓
Wizard requests restart
  ↓
Automatic first-boot continuation
  ↓
Automatic backend health convergence
  ↓
Node Ready for Rental
```

---

## 4. Explicit non-goals for the normal wizard path

The normal wizard must not require the operator to:

- separately launch Host Control;
- run PowerShell, CMD, SSH, or a CLI command;
- paste a setup token twice;
- set up FRP manually;
- configure router port forwarding, UPnP, or DMZ;
- start Host, Sentinel, or Keeper manually;
- inspect logs to decide whether the node is ready;
- enter a Host Control password after the installation token was accepted.

Advanced diagnostic/recovery tools may exist, but they are outside the normal operator flow.

---

## 5. Current-gap rule

Any implementation that stops at one of these states is **not done** against this blueprint:

- bundle merely downloaded or validated;
- package merely staged;
- activation token merely accepted;
- `ARMED`;
- `PROVISIONED_WAITING_HEARTBEATS`;
- a local service exists but backend does not report readiness;
- Host Control must still be opened to complete normal setup;
- `rentalReady: false`.

---

## 6. Required evidence for release acceptance

A release claiming compliance must provide a clean-machine test record proving:

1. installer starts from one `Setup.exe`;
2. folder selection is visible and honored;
3. token entry/validation occurs inside the wizard;
4. invalid token produces inline error and no ready node;
5. valid token provisions Host, Sentinel, and Keeper without opening Host Control;
6. any required reboot resumes automatically;
7. final backend-observed state is `READY` / `rentalReady: true`;
8. a real Direct P2P and explicit Relay path can subsequently be selected and reported honestly;
9. no secrets were emitted to logs, command line, result files, or UI;
10. a controlled failure/retry neither duplicates a license binding nor creates a second charge.

---

## 7. Implementation freedom

This document specifies the operator outcome and safety boundaries, not a mandated internal architecture. The implementation may use installers, wrappers, bootstrap services, scheduled tasks, state machines, service orchestration, or other appropriate mechanisms, provided every acceptance criterion above remains true.

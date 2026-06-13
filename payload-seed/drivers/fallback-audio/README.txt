Fallback audio bundle contract
==============================

Purpose
-------
This folder is the clean-host fallback path for Windows 10 or any host where the
signed "Virtual Audio Driver by MTT" package does not become active automatically.
The bundled custom package should expose SYMO-friendly endpoint names:

- `SYMO Virtual Audio Output`
- `SYMO Virtual Audio Input`

How host-installer uses this folder
-----------------------------------
`prepare-host` only tries this fallback path when:

1. The current host does not already have a working virtual audio route.
2. The MTT route is unavailable, unsupported, or failed to activate.
3. `drivers/fallback-audio/install-audio.ps1` exists AND this folder also contains:
   - `package.json`, or
   - a `payload` folder, or
   - an installer file (`.exe`, `.msi`, `.inf`, `.zip`)

What you need to add for a clean Windows 10 bundle
--------------------------------------------------
1. Put the fallback installer payload in `drivers/fallback-audio/payload/`
2. Copy `package.template.json` to `package.json`
3. Edit `package.json`:
   - `installer`: relative path to the payload installer
   - `package_source`: e.g. `symo-production-signed`
   - `signing_mode`: e.g. `production-signed`
   - `legacy_hardware_id`: required for legacy INF packages that need `devcon install`
   - `arguments`: silent install arguments
   - `post_install_delay_seconds`: optional settle time after install
   - `expected_audio_endpoints`: the output/input names that must appear after install
   - `verify_timeout_seconds`: how long the script should wait for those endpoints
   - `signing_metadata`: optional path to imported signing metadata

Examples
--------
If you bundle a silent EXE:

  installer: payload\\MyAudioDriverSetup.exe
  arguments: ["/S"]

If you bundle an MSI:

  installer: payload\\MyAudioDriver.msi
  arguments: ["/qn", "/norestart"]

Verification
------------
`install-audio.ps1` now verifies the expected endpoints after install by polling
Windows audio device inventory. The script fails if the driver exits cleanly but
`SYMO Virtual Audio Output` and `SYMO Virtual Audio Input` do not appear before
the timeout.

Notes
-----
- Windows 11 (build 22000+) should prefer the signed MTT path automatically.
- This fallback path is mainly for clean Windows 10 hosts.
- `prepare-host` will refresh host capability after the script exits and then
  auto-select speaker/output + mic/input if the driver exposes valid endpoints.
- If the bundled `SYMO` package is only test-signed, Windows 10 may create the
  legacy device but reject the kernel driver with Code 52. In that case you need
  test-signing mode for internal testing or a production-signed `SYMO` package.
- Production signing workflow is documented in:
  `repo\readme\symo_audio_production_signing.md`

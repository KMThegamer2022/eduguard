# EduGuard add-ons for Home Assistant OS

An [add-on repository](https://developers.home-assistant.io/docs/add-ons/configuration#add-on-repository)
for EduGuard, a classroom safeguarding/monitoring suite.

Contains:

- **[EduGuard Server](eduguard-server/)** — the EduGuard management server
  (browser console + device gateway) that Windows/macOS/Linux agents enroll
  against.

## Install

1. In Home Assistant: **Settings → Add-ons → Add-on Store → ⋮ (top right) →
   Repositories**.
2. Paste `https://github.com/KMThegamer2022/eduguard-addons` and **Add**.
3. Open **EduGuard Server** → **Install** (the image is built on the device,
   takes a minute) → set the options → **Start**.
4. The first-run **admin password** and **enrollment token** are printed in
   the add-on's **Log** tab. Console: `http://<home-assistant-ip>:8443/`.

Architectures: `aarch64` and `amd64`. All data (config, SQLite database,
screenshots) lives in the add-on's `/data` and is included in Home Assistant
backups.

> The EduGuard **agent** does not run on Home Assistant OS — it is a headless
> appliance with no student session, screen or tray to manage. Enroll real
> Windows/macOS/Linux devices against this server instead.

## How this repository is maintained

Everything here is generated from `packaging/homeassistant/` in the EduGuard
project; the binaries come from `scripts/build-all.sh` +
`scripts/package-all.sh`. To publish an update:

```sh
./scripts/build-all.sh
./scripts/package-all.sh
./scripts/publish-addon-repo.sh   # syncs, bumps the version, pushes
```

Home Assistant then offers the update after **⋮ → Check for updates** in the
Add-on Store (updates are detected by the `version:` in the add-on's
`config.yaml`).

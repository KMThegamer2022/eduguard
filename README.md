# EduGuard

> ⚠️ **Status: work in progress.** EduGuard is under active development —
> expect rough edges, incomplete features and breaking changes between
> versions. Preview installers for every platform are attached to the
> [Releases](https://github.com/KMThegamer2022/eduguard/releases) page.

Cross-platform classroom safeguarding suite (Impero-style): a management
server with a browser console, plus device agents for **Windows, macOS and
Linux** that report activity, enforce classroom policy (blank / lock / mute),
scan for safeguarding keywords, and keep a full audit trail.

Pure Rust workspace:

| crate            | role                                                        |
|------------------|-------------------------------------------------------------|
| `protocol`       | shared JSON WebSocket protocol (v1)                          |
| `eduguard-server`| axum + SQLite: console, device gateway, keyword engine, audit |
| `eduguard-agent` | device agent: capture, policy enforcement, overlay + tray    |

The web console is dependency-free vanilla JS embedded in the server binary —
a single file deploys.

## Building

```sh
cargo build --release                      # host binaries
cargo test --workspace                     # unit tests

scripts/build-all.sh all                   # every target -> dist/bin/
scripts/package-all.sh                     # installers     -> dist/
```

`build-all.sh` targets and their toolchains:

- **host** — plain cargo.
- **macos** — cargo + `lipo` universal (arm64 + x86_64) build.
- **linux** — `cargo-zigbuild` with zig (glibc 2.17 baseline); cross C code
  (bundled SQLite, aws-lc) goes through `scripts/zig-cc` / `scripts/zig-ar`.
- **windows** — `x86_64-w64-mingw32-gcc`.

Toolchains on macOS: `brew install zig cargo-zigbuild mingw-w64 makensis
dpkg-deb rpm`, plus `rustup target add x86_64-pc-windows-gnu
x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin`.

### Linux capture note

The agent's Linux capture stack is a vendored `xcap`
(`vendor/xcap`, see `EDUGUARD.md` there): **X11/xcb only** — zero native
runtime dependencies. On Wayland sessions screenshots return an explicit error
(logged; the agent keeps running). Window titles are X11-only as upstream.

## Installing

### Linux (systemd)

```sh
# tarball
sudo ./install.sh server       # or: install.sh agent

# deb / rpm
sudo dpkg -i eduguard-server-0.1.0-amd64.deb
sudo rpm -i eduguard-server-0.1.0-1.x86_64.rpm
```

The server starts at boot (`eduguard-server.service`). First-run credentials:

```sh
journalctl -u eduguard-server | grep 'first run'
# admin password + enrollment token; also: eduguard-server reset-admin-password
```

Enroll a device:

```sh
sudo eduguard-agent enroll --server http://SERVER:8443 --token eg-...
sudo systemctl start eduguard-agent
```

X11 capture/overlay for the service can need the seat's display:

```sh
sudo tee /etc/eduguard/agent.env <<<'DISPLAY=:0
XAUTHORITY=/home/<user>/.Xauthority'
```

Linux has **no tray icon** (deliberate; documented limitation). Overlay and
screenshots need an X11 session.

### macOS

```sh
sudo installer -pkg eduguard-0.1.0-macos-universal.pkg -target /
```

Installs `/usr/local/bin/eduguard-{server,agent}` plus launchd jobs
(`com.eduguard.server` LaunchDaemon, `com.eduguard.agent` LaunchAgent with a
🛡 menu-bar icon: connection status + "Open Console").

```sh
sudo /usr/local/bin/eduguard-server reset-admin-password     # console login
sudo /usr/local/bin/eduguard-agent enroll --server http://SERVER:8443 --token eg-...
```

**Privacy grant:** System Settings → Privacy & Security → grant *Screen
Recording* (screenshots) and *Accessibility* (blanking) to the agent binary.
Until granted, live view reports "no screen yet" honestly.

### Home Assistant OS

Home Assistant OS is container-first: its host filesystem is read-only and
wipes on update, so the server ships as an **add-on** (a Docker image the
Supervisor builds on-device) instead of host binaries:

1. In Home Assistant: **Settings → Add-ons → Add-on Store → ⋮ → Repositories**
   → add `https://github.com/KMThegamer2022/eduguard-addons`.
2. Install **EduGuard Server** (the Supervisor builds the image on-device)
   and start it. *Local variant:* copy `packaging/homeassistant/eduguard-server/`
   to `/addons/eduguard-server` on the device instead — `config.yaml` must sit
   directly at `/addons/eduguard-server/config.yaml`, not nested one level deeper.
3. Console: `http://<home-assistant-ip>:8443/` — the first-run admin password
   and enrollment token are printed in the add-on **Log** tab.

Configuration and data live in the add-on's `/data` (included in Home
Assistant backups). See `packaging/homeassistant/eduguard-server/README.md`.

The **agent does not run on Home Assistant OS** — it is a headless appliance
with no student session, screen or tray to manage. Enroll real
Windows/macOS/Linux devices against this server instead. For plain Docker on
any other host: `packaging/docker/docker-compose.yml`.

### Windows

```powershell
.\eduguard-server-0.1.0-setup.exe          # GUI installer
# or unattended:
.\install.ps1 -Component server            # exe + install.ps1 in the zip

.\eduguard-agent-0.1.0-setup.exe /S /SERVER=http://server:8443 /TOKEN=eg-...
# or:
.\install.ps1 -Component agent -ServerUrl http://server:8443 -Token eg-...
```

The server runs as SYSTEM at boot via the *EduGuard Server* scheduled task
(console app, not a service) and opens firewall port 8443. The agent starts at
each user logon (HKLM Run key) so its tray icon and overlay run inside the
interactive session. Console: `http://localhost:8443/`, user `admin`, password
from the installer summary or `C:\ProgramData\EduGuard\server\server.toml`.

## Console

- **Devices grid** — live screen previews, refreshed about every 2 s while the
  grid is visible (`/api/viewing` keeps agent FPS up).
- **Device details** — live screen view at the top, same 2 s cadence, while
  the dialog is open.
- Screenshots are authenticated with `?token=` (CSS background-images cannot
  send an `Authorization` header); the token is validated like any session
  credential.
- Activity log (foreground apps), keyword alerts (self-harm etc.), staff audit
  trail, bulk lock/blank/screenshot.

## Discreet by default

The agent is quiet by default — service + tray icon only, like Impero:

- no on-screen "monitored" banner (`monitor_banner: false`)
- no capture-evidence indicator (`capture_indicator: false`)

Both are opt-in per device or in the console's default-policy settings.
Screenshot capture is *not* pre-announced on screen unless the indicator is
enabled — check your local policy/Safeguarding obligations before deploying.

## Paths

|                   | Linux                          | macOS                                | Windows                          |
|-------------------|--------------------------------|--------------------------------------|----------------------------------|
| config            | `/etc/eduguard/*.toml`         | same                                 | `C:\ProgramData\EduGuard\*`      |
| data (SQLite + screenshots) | `/var/lib/eduguard` | `/Library/Application Support/EduGuard` | `C:\ProgramData\EduGuard\server` |

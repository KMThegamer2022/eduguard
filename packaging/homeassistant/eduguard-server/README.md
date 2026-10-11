# EduGuard Server add-on for Home Assistant OS

Runs the EduGuard management server (web console + device gateway) as a
container on Home Assistant OS. Classroom devices — Windows, macOS and
Linux machines — run the EduGuard *agent* and enroll against this server.

> The **agent does not run on Home Assistant OS** itself: HAOS is a headless
> appliance with no desktop session to monitor (no tray, overlay or screen
> capture). Put the agent on the devices you actually want to supervise.

## Install

**Local add-on (no git hosting needed):**

1. Copy this folder to `<homeassistant>/addons/eduguard-server`
   (via the Samba or SSH add-on; the share is usually
   `/mnt/data/supervisor/addons`).
2. Settings → Add-ons → Add-on Store → **Local add-ons** → *EduGuard Server* →
   **Build** → **Install** → **Start**.

**As a repository (recommended):** Add-on Store → ⋮ → *Repositories* →
paste `https://github.com/KMThegamer2022/eduguard-addons` — the Supervisor
fetches the add-on from GitHub, no file copying needed.

## Configuration (first run)

| option            | default          | notes                                   |
|-------------------|------------------|-----------------------------------------|
| `admin_password`  | *(generated)*    | left empty → random, shown in the log   |
| `school_name`     | `EduGuard School`| shown in student-facing overlays        |

Options are written to `/data/server.toml` on **first run only**. To re-apply
changed options: stop the add-on, delete `/data/server.toml`, start it again —
the device database and screenshots in `/data` are preserved.

## First steps

1. Open `http://<home-assistant-ip>:8443/` — log in as `admin`.
2. Check the add-on **Log** tab: the first run prints
   `[eduguard] first run -> admin password: …` and a default
   `enrollment token: eg-…` (or create tokens later in
   Console → Settings → Enrollment tokens).
3. On each classroom device, install the EduGuard agent and run:

   ```
   eduguard-agent enroll --server http://<home-assistant-ip>:8443 --token eg-...
   ```

## Notes

- Architectures: `aarch64` and `amd64` (armv7/i386 Home Assistant devices are
  not supported — the add-on will refuse to start rather than run half-working).
- Persistent data lives entirely in `/data` (config, SQLite, screenshots) and
  is included in Home Assistant backups automatically.
- Plain HTTP on port 8443 (typical LAN deployment). For TLS, point
  `tls_cert`/`tls_key` in `server.toml` at PEM files you place in `/data`.

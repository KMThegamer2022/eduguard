# GOAL: Rebuild Impero Education Pro as a Modern, Cross-Platform, Open Suite

**Codename:** EduGuard *(note: "Impero" is a registered trademark — this project must ship under an original name and must not copy Impero's branding, logos, artwork, or proprietary keyword libraries.)*

**One-line goal:**
> Build a from-scratch, self-hosted classroom management + device management + safeguarding suite that reaches ~95% feature parity with Impero Education Pro, runs a **first-class Linux server** and **first-class Linux/macOS/Windows clients**, and improves on Impero with modern OS support, modern security, and a modern web-based console.

---

## 1. Why / Vision

Impero Education Pro is a three-in-one product: **classroom management**, **network/device management**, and **online-safety (safeguarding) monitoring** — all from one console, on-premises, cross-platform. Its weaknesses today:

- Server and admin tooling are Windows-centric.
- Client support for Linux is weak/absent; macOS and ChromeOS support lags current OS APIs (TCC, ScreenCaptureKit, Wayland).
- Filtering depends on browser extensions and legacy interception rather than modern identity-aware enforcement.
- Reporting is split (console reports + separate "Insight" BI module).
- UI/UX, deployment (MDM/Intune/GPO), and integration (SSO, rostering, LMS) are dated.

**Our target:** one coherent suite, one data model, one console, native on current Windows 11/12, macOS 14+, and Linux (Ubuntu 22.04+/Debian 12+/RHEL 9+/ChromeOS-by-Linux-container where feasible), with the server deployable as a Linux package, Docker Compose, or Kubernetes chart.

---

## 2. Feature Parity Matrix (the core requirement)

Everything below must exist and behave comparably to Impero, unless marked *NEW* or *MODERNIZED*.

### A. Safeguarding / Online Safety
| # | Feature | Notes |
|---|---------|-------|
| A1 | **Keyword detection** — typed text, on-screen text (OCR), URLs, app content, email/chat | Detect in browsers, desktop apps, terminals; configurable scope |
| A2 | **Keyword libraries** — categorized sets (self-harm, bullying, extremism, etc.) with contextual definitions | Ship *our own* libraries; import/export format compatible in *structure* only |
| A3 | **Real-time alerting** — live alert feed to designated staff, email/webhook alerts | Alerts must include context snippet, user, device, severity |
| A4 | **Activity logs & incident handling** — searchable history, screenshot/video evidence capture, incident workflow (flag → assign → review → resolve → report) | Retention policies configurable per policy |
| A5 | **Anonymous pupil reporting tool** ("Confide"-equivalent) — student submits concern to a trusted staff member | *NEW:* web form + optional anonymity, abuse-rate limiting |
| A6 | **Acceptable-use policy enforcement** — show/require acknowledgement at login | Per-group, per-device variants |
| A7 | **Access management** — restrict/allow web, apps, and resources by user, group, room, device, or live session | Immediate ("on-the-fly") and scheduled |
| A8 | **USB / peripheral control** — track, allow, block, disable USB storage (and per-type policies) | Windows + macOS + Linux implementations |
| A9 | **Screen capture evidence** — timed screenshot/video recording of a device on alert | Hardware-encoded; strict audit trail of *who captured what* |

### B. Classroom Management (Teacher Console)
| # | Feature | Notes |
|---|---------|-------|
| B1 | **Live thumbnails** of every student screen, grouped by class/room, with large-grid/list views | Adaptive FPS/bandwidth |
| B2 | **Screen broadcasting** — teacher→class, teacher→selected, student→class (with teacher approval) | Low latency, hardware encode |
| B3 | **Remote control** of a student device (with visible indicator + audit log) | Full control, view-only, and "request control" from student |
| B4 | **Teaching tools (one-click)** — lock screen, blank screens, disable internet, mute sound, restrict printing, disable keyboard/mouse, clear clipboard | Bulk and per-student |
| B5 | **Messaging & live chat** — 1:1, group, whole-class broadcast; student-initiated questions | Read receipts, staff-only channels |
| B6 | **File transfer** — send file/folder to students, collect student work, push & run a file or URL on devices | Quotas, malware-scan hook (*NEW*) |
| B7 | **Grouping** — class rosters, ad-hoc groups, **PIN grouping** for BYOD, cross-platform membership | Synced to all participants |
| B8 | **Assessment tools** — exam/lockdown mode (secure browser, cut/paste/switch disabled), quick question, quick quiz, task assignment with student "mark complete" | Quiz auto-grade; exam log |
| B9 | **Attention/raise-hand** — students request help, teacher queue | *NEW* but expected in modern tools |
| B10 | **Attendance / presence** — auto-detect device+user online state per session | *NEW*, feeds reports |

### C. Network / Device Management (Admin Console)
| # | Feature | Notes |
|---|---------|-------|
| C1 | **Usage logging** — sites visited, windows/apps used, print jobs, files deleted, login sessions | Central store; per-user and per-device views |
| C2 | **Hardware & software inventory** — auto-discovery, installed apps, versions, drivers, peripherals | Scheduled + on-change scans |
| C3 | **Software license management** — entitlement tracking, usage vs. seats, alerts | *MODERNIZED:* import from SPDX/SBOM where possible |
| C4 | **Power management** — schedules, idle shutdown, Wake-on-LAN, energy/cost savings report | Per-room schedules |
| C5 | **Print management** — job logging, quotas, cost accounting, printer restrictions | Pull-print integration via *NEW* optional follow-you release |
| C6 | **Web filtering / internet restriction** — category/URL/domain/CERT allow/deny lists, safe-search enforcement, time-of-day rules | *MODERNIZED:* see §4 |
| C7 | **Application control** — block/allow/detect executables and window titles, with actions (log, warn, logout, ban internet, alert) | Includes script/PowerShell policy hooks on Windows |
| C8 | **Remote actions** — shutdown, restart, log off, sleep/wake, run command/script, deploy MSI/pkg/DEB | Scheduled + immediate |
| C9 | **Bandwidth control** (optional) — per-client rate limiting on peak hours | *NEW*, best-effort |
| C10 | **Multi-site / multi-server management** — hierarchical sites, rooms, device groups | *MODERNIZED:* replaces "multi-server reporting" |

### D. Reporting & Analytics
| # | Feature | Notes |
|---|---------|-------|
| D1 | **Built-in reports** — activity, safeguarding, inventory, print, power, license, class engagement | CSV/PDF export, scheduled email delivery |
| D2 | **Interactive dashboards** (Insight-equivalent) — drill-down from school → site → room → device → user → event | *MODERNIZED:* one system, not a separate BI product; charts server-side rendered |
| D3 | **Role-based report permissions** — safeguarding leads see safeguarding data; teachers see only their classes | Enforced server-side |
| D4 | **Audit trail of staff actions** — every remote control, capture, unlock, policy change is logged and reportable | Non-repudiation: signed append-only audit log |
| D5 | *NEW:* **Exports & API** — REST/GraphQL + webhooks so data can feed SEPA/State reporting or a SIEM | |

### E. Administration
| # | Feature | Notes |
|---|---------|-------|
| E1 | **Roles & permissions** — Super Admin, IT Admin, Safeguarding Lead, Teacher, TA, Read-only Auditor | |
| E2 | **Directory integration** — AD/LDAP + *NEW* OIDC SSO (Entra/Google/Okta) + SCIM provisioning | |
| E3 | **Roster/LMS integration** — Google Classroom, Canvas, Moodle, Office 365 Groups via LTI/roster API | *NEW* |
| E4 | **Policy engine** — layered policies (site → room → group → device → user), precedence rules, inheritance, dry-run preview | *MODERNIZED:* single source of truth for A7/C6/C7 |
| E5 | **Branding/white-label** — school name, logo, colors on student-facing UI | |
| E6 | **Backup/restore, HA, upgrade-in-place** | Linux-native tooling |

---

## 3. Target Architecture

```
                       ┌────────────────────────────────────────────┐
                       │  SERVER (Linux-first)                     │
                       │  eduguard-server                          │
                       │  ├─ API gateway (REST + gRPC/WebSocket)   │
                       │  ├─ Device/session broker                 │
                       │  ├─ Policy engine                         │
                       │  ├─ Capture/keyword pipeline (OCR, scan)  │
                       │  ├─ Scheduler (power, reports, retention) │
                       │  └─ Audit log (append-only, hash-chained) │
                       │  PostgreSQL · S3/MinIO object store       │
                       │  optional: Redis/NATS, Prometheus, SMTP   │
                       └────────────▲───────────────▲──────────────┘
                 mTLS + WebSocket/QUIC (long-lived device channel)
                                    │               │
        ┌───────────────────────────┴──┐   ┌────────┴──────────────────────┐
        │ TEACHER/ADMIN CONSOLE        │   │ DEVICE AGENTS (per endpoint)  │
        │  • Web console (browser)     │   │  eduguard-agent (Rust, ~15MB) │
        │  • Desktop app (Tauri wrap)  │   │  ├─ screen capture/encode     │
        │  • Mobile PWA (view + alert) │   │  ├─ input lock, app/web ctrl  │
        └──────────────────────────────┘   │  ├─ input hook, OCR, inventory│
                                           │  └─ system service/daemon     │
                                           └───────────────────────────────┘
```

**Principles**
1. **Server is Linux-native** — runs on bare metal, `.deb`/`.rpm`, Docker Compose (default install), or Helm chart.
2. **Agent is a single static binary** — small RSS footprint, works on Windows/macOS/Linux with platform-specific backends behind one interface.
3. **Console is web-first** — same UI in browser and wrapped desktop app; no per-OS console builds.
4. **Everything is mTLS device identity** — enrolled device gets a cert; no shared secrets in configs.
5. **One policy engine** — filtering, app control, and classroom restrictions all read the same policy tree.
6. **Offline-tolerant** — devices queue events and keep enforcing last-known policy if the server drops.

---

## 4. Modernization & Improvement List (the "some things updated")

- **Wayland support** (Linux): capture via `pipewire`/`xdg-desktop-portal`, input control via a privileged `evdev`/`libinput` helper — X11 fallback kept.
- **Modern macOS support**: ScreenCaptureKit + CGEvent for input, TCC permission bootstrap with MDM-configured profiles, Notarized `.pkg`, MDM/DEP zero-touch enrollment, focus on macOS 14+.
- **Modern Windows support**: WinRT/GDI/DXGI capture with hardware encode, Windows 11 validation, GPO + Intune + `.msix` deployment, per-user *and* SYSTEM service modes.
- **Modern filtering** — replace extension-only interception with: extension (Chromium/Firefox) for per-tab policy + **explicit proxy with MITM opt-in** + DNS/DoH redirection; *NEW:* teacher-triggered "block this tab now" without proxy.
- **Hardware video encoding** — H.264/HEVC via NVENC/AMF/VideoToolbox/VAAPI; VP9/AV1 fallback. Target: thumbnail grid of 40 devices at <1.5 Mbps per viewer.
- **SSO + SCIM + LMS rostering** (E2/E3) — replaces imperative per-user setup.
- **Privacy by design**: per-role data scoping, configurable retention (e.g., screenshots 30 days), PII pseudonymization in analytics view, student-visible "you are being monitored" banner (honesty default ON), full staff-action audit trail.
- **Compliance pack**: GDPR (DSR export/delete), UK KCSIE + Keeping Children Safe, US COPPA/FERPA, state-specific rules toggles.
- **Deployment**: one-command `docker compose up`, ARM64 builds (Raspberry Pi / Apple Silicon), air-gapped/offline installer.
- **Extensibility**: webhook + REST API + plugin sandbox for custom keyword sources and alert sinks (Teams/Slack/PagerDuty).
- **AI-assisted triage (opt-in, on-prem only)**: cluster/dedupe alerts, summarize incident timelines — never auto-escalate a student; human-in-the-loop only.
- **Observability**: built-in health dashboard, Prometheus metrics, structured logs.
- **Accessibility & i18n**: WCAG 2.2 AA console, RTL-ready, shipped in English + Spanish + French + German + Arabic initially (Impero ships 7 languages).

---

## 5. Platform Support Matrix

| Component | Linux | Windows 10/11 | macOS 14+ | ChromeOS | iOS/Android |
|---|---|---|---|---|---|
| **Server** | ✅ primary (x86_64, arm64) | ⚠️ via WSL2/Docker only | ⚠️ via Docker (dev) | — | — |
| **Device agent** | ✅ (X11 + Wayland) | ✅ | ✅ | ⚠️ phase 6 (container/ARC limits) | ⚠️ phase 6 (MDM-restricted feature set) |
| **Teacher console** | ✅ web + desktop | ✅ web + desktop | ✅ web + desktop | ✅ web | ✅ responsive PWA |
| **Remote control input** | ✅ evdev helper | ✅ SendInput | ✅ CGEvent (TCC-granted) | ❌ not possible | ❌ view-only |
| **Screen capture** | ✅ PipeWire/X11 | ✅ DXGI | ✅ ScreenCaptureKit | ⚠️ tab-only via extension | ⚠️ limited by OS policy |

Feature-gating must be **explicit in the UI**: unsupported actions are disabled with a tooltip explaining *why*, never silently fail.

---

## 6. Recommended Tech Stack (decisive default)

| Layer | Choice | Rationale |
|---|---|---|
| Agent + server core | **Rust** (tokio, axum, tonic) | One language, tiny fast binaries, memory safety in privileged code |
| Console UI | **React + TypeScript + Vite** in **Tauri 2** (desktop) and plain browser (web) | Single UI, small binaries vs Electron |
| Transport | gRPC (commands) + WebSocket/QUIC streams (live video/telemetry), **mTLS** everywhere | Low-latency bidirectional |
| Database | **PostgreSQL 16** (append-only audit tables + JSONB policies) | Ubiquitous, Linux-native |
| Object storage | S3-compatible (**MinIO** bundled, AWS/GCS optional) | Screenshots/video |
| Video | WebCodecs in browser; fMP4 segments over HTTP for compatibility | |
| Queues | Embedded **NATS** (optional component) | Simple, no Kafka tax |
| Packaging | `.deb`/`.rpm`, Docker Compose, Helm, MSI, `.pkg`, AppImage/Flatpak | |
| CI/CD | GitHub Actions matrix builds, signed artifacts (Sigstore/cosign + Authenticode + Notarization) | |

*(Alternative if the team is .NET-fluent: ASP.NET Core 8 server + Rust agent — equally viable.)*

---

## 7. Repository Layout

```
eduguard/
├── GOAL.md                  # this document
├── docs/
│   ├── architecture.md
│   ├── feature-parity.md    # living checklist vs §2
│   ├── security.md          # threat model, TLP, key management
│   ├── compliance/          # GDPR, KCSIE, FERPA notes
│   └── api/
├── crates/
│   ├── agent/               # cross-platform device agent
│   │   ├── platform-linux/  # X11 + Wayland backends, evdev helper
│   │   ├── platform-windows/
│   │   └── platform-macos/
│   ├── server/              # API, broker, policy engine, pipelines
│   ├── protocol/            # protobuf/shared types (versioned!)
│   ├── policy/              # policy tree evaluation
│   └── capture/             # screen capture + encoder abstraction
├── console/                 # React/TS UI (web + Tauri shell)
├── packaging/
│   ├── linux/  (deb, rpm, compose, helm, systemd units)
│   ├── windows/ (msi, intune, gpo adml)
│   └── macos/   (pkg, mdm profile, scripts)
└── tools/ (cli, bulk-enroll, report-export)
```

---

## 8. Security & Ethics Guardrails (non-negotiable)

This software can monitor minors and control their devices. Treat it as **high-impact, privileged software**:

1. **Consent & transparency** — monitoring banner shown to students by default; configuration to disable requires a documented reason in the audit log.
2. **Least privilege** — the agent runs as a service but every *staff* action is scoped by role and logged; no undocumented remote-code-execution channel (remote script execution runs from an allowlisted script library, not arbitrary shell).
3. **No covert capability** — no hidden keystroke logging for its own sake; keyword detection produces *alerts with context*, not full keylogs. (Where regulations require more, it must be an explicit, documented, policy-gated mode.)
4. **Data protection** — encrypted at rest (per-tenant key), TLS 1.3 in transit, retention + auto-purge, DSR export/delete endpoints, regional data residency.
5. **Independent security review** before any pilot with real students (threat model in `docs/security.md`, plus external pentest).
6. **Supply chain** — pinned deps, SBOM per release, signed builds.

---

## 9. Milestones

| Phase | Scope | Exit criteria |
|---|---|---|
| **M0 — Foundations (wk 1–4)** | Repo, protocol, mTLS enrollment, agent skeleton on 3 OSes, server + Postgres in Docker, CI, signing | Enrolled Linux/Win/mac device appears in web console; heartbeat + inventory stub |
| **M1 — Visibility MVP (wk 5–10)** | Screen capture + live thumbnails, grid view, device/user identity, basic usage logging | 40-device grid at target FPS/bandwidth on LAN; logs queryable |
| **M2 — Classroom Control (wk 11–18)** | Lock/blank/mute/net-off, remote control, broadcast, messaging, file push/collect, groups + PIN grouping | Teacher runs a full lesson without touching the lab machine |
| **M3 — Filtering & App Control (wk 19–26)** | Policy engine, browser extension, URL/category filtering, app blocking, USB control, access mgmt on-the-fly | Policy changes apply to live session < 2 s |
| **M4 — Safeguarding (wk 27–34)** | Keyword pipeline + own libraries, OCR, alert feed, incident workflow, capture evidence, anonymous reporting, AUP | End-to-end alert → incident → report on all 3 OSes |
| **M5 — Device Management (wk 35–42)** | Inventory, licenses, power schedules + WoL, print mgmt, remote deploy/remote actions, multi-site | Scheduled shutdown/wake verified in lab; inventory diffing works |
| **M6 — Reporting & Polish (wk 43–50)** | Dashboards, role-scoped reports, scheduled exports, audit report, i18n, a11y pass, docs, install guides | Parity checklist §2 ≥ 95% items "done" |
| **M7 — Hardening (wk 51–56)** | External pentest, load test (1k devices/server), failover, upgrade path, pilot with 1 school | Sign-off; 2-week pilot with zero P1s |

*ChromeOS/iOS/Android agents, HA clustering, and bandwidth shaping are post-1.0 stretch items.*

---

## 10. Success Criteria (Definition of Done)

**Parity:** ≥95% of §2 features implemented and demonstrated side-by-side with Impero behavior.

**Performance budgets:**
- Agent idle CPU < 2% of one core; RAM < 120 MB (excluding encoder buffers).
- Command (e.g., *lock screens*) → visible effect on 40 devices: **< 500 ms** LAN, **< 1.5 s** WAN.
- Live thumbnail latency < 500 ms; server handles **1,000 concurrent devices** on recommended hardware.
- Console first paint < 1.5 s on 100 Mbit LAN.

**Quality:** zero known data-loss paths for audit logs; automated tests incl. protocol-compat suite; runs on clean installs of Ubuntu 24.04, Debian 12, RHEL 9, Windows 11 23H2+, macOS 14/15.

**Ops:** new-school install ≤ 30 minutes (server + 30 agents) following docs alone; upgrade in place without re-enrolling devices.

**Trust:** published security whitepaper, compliance docs, and a visible transparency report of staff monitoring actions.

---

## 11. Immediate Next Actions

1. Rename/confirm project identity (avoid "Impero" in any shipped artifact).
2. Write `docs/architecture.md` + freeze `protocol/*.proto` v0 (versioned, forward-compatible).
3. Scaffold the Cargo workspace and CI matrix (Linux/macOS/Windows builds).
4. Build M0: enrollment + heartbeat + inventory stub end-to-end.
5. Stand up a 3-device lab (Ubuntu 24.04 Wayland, Windows 11, macOS 15) — it becomes the parity test bench.
6. Maintain `docs/feature-parity.md` as the living checklist against §2 — this is the scoreboard for "almost exactly."

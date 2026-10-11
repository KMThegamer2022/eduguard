#!/bin/sh
# EduGuard tarball installer (systemd-based Linux).
#
# Usage:
#   sudo ./install.sh            # component auto-detected from archive contents
#   sudo ./install.sh server
#   sudo ./install.sh agent
#
# Archives contain: eduguard-server|eduguard-agent, the matching .service
# unit, and this script.
set -eu

[ "$(id -u)" -eq 0 ] || { echo "error: run as root (sudo ./install.sh)" >&2; exit 1; }

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
comp="${1:-}"
if [ -z "$comp" ]; then
    if [ -f "$here/eduguard-server" ]; then
        comp=server
    elif [ -f "$here/eduguard-agent" ]; then
        comp=agent
    else
        echo "error: no eduguard-server/eduguard-agent found in $here" >&2
        exit 1
    fi
fi

install -d /etc/eduguard /var/lib/eduguard

case "$comp" in
server)
    install -m755 "$here/eduguard-server" /usr/bin/eduguard-server
    install -m644 "$here/eduguard-server.service" /etc/systemd/system/eduguard-server.service
    systemctl daemon-reload
    systemctl enable eduguard-server >/dev/null 2>&1 || true
    systemctl restart eduguard-server >/dev/null 2>&1 || systemctl start eduguard-server >/dev/null 2>&1 || true
    echo "EduGuard server installed."
    echo "  Console:  http://$(hostname -f 2>/dev/null || hostname):8443/"
    echo "  First-run admin password + enrollment token:"
    echo "    journalctl -u eduguard-server | grep 'first run'"
    echo "  (also stored in /etc/eduguard/server.toml)"
    ;;
agent)
    install -m755 "$here/eduguard-agent" /usr/bin/eduguard-agent
    install -m644 "$here/eduguard-agent.service" /etc/systemd/system/eduguard-agent.service
    systemctl daemon-reload
    systemctl enable eduguard-agent >/dev/null 2>&1 || true
    echo "EduGuard agent installed (enabled; starts after enrollment)."
    echo "  Enroll: sudo eduguard-agent enroll --server http://SERVER:8443 --token eg-..."
    echo "  Then:   sudo systemctl start eduguard-agent"
    echo "  X11 capture/overlay: set DISPLAY/XAUTHORITY in /etc/eduguard/agent.env"
    ;;
*)
    echo "error: unknown component '$comp' (expected: server | agent)" >&2
    exit 1
    ;;
esac

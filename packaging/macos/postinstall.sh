#!/bin/sh
# EduGuard macOS pkg postinstall: load the launchd jobs that were just
# placed in /Library/LaunchDaemons (server) and /Library/LaunchAgents (agent).
set -e

chmod 644 /Library/LaunchDaemons/com.eduguard.server.plist \
          /Library/LaunchAgents/com.eduguard.agent.plist
chown root:wheel /Library/LaunchDaemons/com.eduguard.server.plist

# Server: system daemon, starts now.
launchctl bootstrap system /Library/LaunchDaemons/com.eduguard.server.plist 2>/dev/null || \
  launchctl load -w /Library/LaunchDaemons/com.eduguard.server.plist 2>/dev/null || true

# Agent: load into the console user's GUI session (tray + overlay need one).
uid=$(stat -f%u /dev/console 2>/dev/null || echo 0)
if [ -n "$uid" ] && [ "$uid" != "0" ]; then
  launchctl bootstrap "gui/$uid" /Library/LaunchAgents/com.eduguard.agent.plist 2>/dev/null || \
    launchctl load -w /Library/LaunchAgents/com.eduguard.agent.plist 2>/dev/null || true
fi

echo "EduGuard installed."
echo "  Console:  http://localhost:8443/"
echo "  Password: sudo /usr/local/bin/eduguard-server reset-admin-password"
echo "  Enroll:   sudo /usr/local/bin/eduguard-agent enroll --server http://SERVER:8443 --token eg-..."
echo "  macOS note: grant Screen Recording + Accessibility to the agent binary"
echo "  in System Settings > Privacy & Security for live screenshots/blanking."
exit 0

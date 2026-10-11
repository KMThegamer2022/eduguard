# EduGuard server add-on.
#
# Binaries are prebuilt by scripts/build-all.sh and staged into bin/ by
# scripts/package-all.sh, so no compiler is needed on the Home Assistant
# device. bookworm's glibc (2.36) covers our 2.17 build baseline. The
# Supervisor builds this image natively on the host (amd64 or aarch64);
# entrypoint.sh picks the matching binary at startup.
FROM debian:bookworm-slim

# jq reads the Supervisor's /data/options.json reliably (quotes, backslashes
# and unicode in passwords would break a hand-rolled sed parser).
RUN apt-get update \
    && apt-get install -y --no-install-recommends jq \
    && rm -rf /var/lib/apt/lists/*

COPY bin /opt/eduguard/bin
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 8443
CMD ["/entrypoint.sh"]

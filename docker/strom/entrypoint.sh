#!/bin/bash
# Entrypoint for strom Docker image
#
# Starts dbus and avahi-daemon for NDI network discovery.
# NDI uses mDNS (Avahi) to discover streams on the local network.
# Without Avahi, NDI sources/sinks work but discovery does not.

# NDI discovery uses Avahi. That daemon needs root. NMOS DNS-SD does not:
# with NMOS_DNS_SD=false (the default) the process never opens mDNS.
# When the image runs as uid 1000, skip Avahi and keep going.
if [ "$(id -u)" -eq 0 ]; then
    rm -f /run/dbus/pid
    mkdir -p /run/dbus
    dbus-daemon --system 2>/dev/null
    rm -f /run/avahi-daemon/pid
    avahi-daemon -D 2>/dev/null
fi

# Replace this shell with the container command so `docker run IMAGE CMD`
# actually runs CMD as PID 1. Empty args fall back to the image default.
if [ "$#" -eq 0 ]; then
    set -- /app/strom
fi
exec "$@"

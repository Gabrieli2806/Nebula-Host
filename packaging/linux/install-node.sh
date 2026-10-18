#!/bin/sh
# Install the headless Nebula Host node on a Linux machine (Raspberry Pi,
# home server, VPS on your LAN) as a systemd service.
#
#   sudo ./install-node.sh            # installs for the user who ran sudo
#   sudo ./install-node.sh --user bob # installs for another user
#
# Afterwards: run `nebula-node pair` and enter the code in Nebula Host
# (machine menu › Connect to another machine).
set -eu

RUN_USER="${SUDO_USER:-$(id -un)}"
while [ $# -gt 0 ]; do
  case "$1" in
    --user) RUN_USER="$2"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

if [ "$(id -u)" -ne 0 ]; then
  echo "Run with sudo: sudo $0" >&2
  exit 1
fi

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN=/usr/local/bin/nebula-node
install -m755 "$HERE/nebula-node" "$BIN"
sed -e "s|__USER__|$RUN_USER|" -e "s|__BIN__|$BIN|" "$HERE/nebula-node.service" > /etc/systemd/system/nebula-node.service
systemctl daemon-reload
systemctl enable --now nebula-node.service

echo
echo "Nebula node installed and running as $RUN_USER."
if command -v ufw >/dev/null 2>&1 && ufw status 2>/dev/null | grep -q "Status: active"; then
  echo "ufw is active: allow the node API from your LAN, e.g."
  echo "  sudo ufw allow from 192.168.1.0/24 to any port 47400 proto tcp"
  echo "and your game ports (25565 for Minecraft)."
fi
if ! command -v java >/dev/null 2>&1; then
  echo "No Java found. Minecraft needs it, e.g.: sudo apt install openjdk-21-jre-headless openjdk-25-jre-headless"
fi
echo "Pair a device:  nebula-node pair"

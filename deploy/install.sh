#!/usr/bin/env bash
# Bootstrap Bot_BN_BBit on Ubuntu (Tokyo observer OR Singapore executor stack).
# Usage (as root):
#   tar -xzf bot-release-*.tar.gz -C /tmp/bot-rel
#   ROLE=tokyo  bash /tmp/bot-rel/install.sh
#   ROLE=singapore bash /tmp/bot-rel/install.sh
set -euo pipefail

ROLE="${ROLE:-}"
if [[ "$ROLE" != "tokyo" && "$ROLE" != "singapore" ]]; then
  echo "Set ROLE=tokyo or ROLE=singapore" >&2
  exit 1
fi

SRC="$(cd "$(dirname "$0")" && pwd)"
id -u bot &>/dev/null || useradd --system --home /opt/bot --shell /usr/sbin/nologin bot
mkdir -p /opt/bot/bin /etc/bot /var/log/bot
chown -R bot:bot /opt/bot /var/log/bot
chmod 750 /etc/bot

install -m 755 "$SRC"/bin/* /opt/bot/bin/ 2>/dev/null || true
# Strip .exe if packaged from Windows
for f in /opt/bot/bin/*.exe; do
  [[ -e "$f" ]] || continue
  mv "$f" "${f%.exe}"
done

install -m 644 "$SRC"/etc/*.toml /etc/bot/ 2>/dev/null || true
# Absolute edge path for WorkingDirectory=/opt/bot
if [[ -f /etc/bot/config.toml ]]; then
  sed -i 's|edge_profile_path = "config/edge_profile.toml"|edge_profile_path = "/etc/bot/edge_profile.toml"|' /etc/bot/config.toml || true
  sed -i 's|edge_profile_path = "config/edge_profile.toml"|edge_profile_path = "/etc/bot/edge_profile.toml"|' /etc/bot/config.testnet.toml 2>/dev/null || true
fi

# Secrets: never overwrite existing
if [[ ! -f /etc/bot/secrets.env ]]; then
  if [[ -f "$SRC/etc/secrets.env.example" ]]; then
    install -m 600 -o bot -g bot "$SRC/etc/secrets.env.example" /etc/bot/secrets.env
    echo "Created /etc/bot/secrets.env — FILL KEYS before start"
  fi
fi

# Zenoh
if [[ "$ROLE" == "tokyo" ]]; then
  if [[ ! -f /etc/bot/zenoh.json5 ]]; then
    install -m 644 "$SRC/etc/zenoh-tokyo.json5.example" /etc/bot/zenoh.json5
  fi
  install -m 644 "$SRC/systemd/observer.service" /etc/systemd/system/observer.service
  systemctl daemon-reload
  systemctl enable observer
  echo "Tokyo ready: edit /etc/bot/secrets.env (optional), systemctl start observer"
elif [[ "$ROLE" == "singapore" ]]; then
  if [[ ! -f /etc/bot/zenoh.json5 ]]; then
    install -m 644 "$SRC/etc/zenoh-singapore.json5.example" /etc/bot/zenoh.json5
    echo "EDIT /etc/bot/zenoh.json5 — set TOKYO_PRIVATE_IP"
  fi
  for u in executor control-panel telegram-alerts; do
    install -m 644 "$SRC/systemd/${u}.service" /etc/systemd/system/${u}.service
  done
  systemctl daemon-reload
  systemctl enable executor control-panel telegram-alerts
  echo "Singapore ready: fill /etc/bot/secrets.env + zenoh peer IP, then start services"
fi

echo "Keep mode=dev in /etc/bot/config.toml until edge pass + testnet smokes OK."

#!/usr/bin/env bash
# Bootstrap Bot_BN_BBit on Ubuntu (Tokyo observer OR Singapore executor stack).
# Usage (as root):
#   tar -xzf bot-release-*.tar.gz -C /tmp/bot-rel && cd /tmp/bot-rel
#   ROLE=tokyo bash install.sh
#   ROLE=singapore PEER_IP=<tokyo_private_ip> bash install.sh
set -euo pipefail

ROLE="${ROLE:-}"
PEER_IP="${PEER_IP:-}"
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
for f in /opt/bot/bin/*.exe; do
  [[ -e "$f" ]] || continue
  mv "$f" "${f%.exe}"
done
chmod 755 /opt/bot/bin/* 2>/dev/null || true

install -m 644 "$SRC"/etc/*.toml /etc/bot/ 2>/dev/null || true
if [[ -f /etc/bot/config.production.toml ]]; then
  install -m 644 /etc/bot/config.production.toml /etc/bot/config.toml
fi
if [[ -f /etc/bot/config.toml ]]; then
  sed -i 's|edge_profile_path = "config/edge_profile.toml"|edge_profile_path = "/etc/bot/edge_profile.toml"|' /etc/bot/config.toml || true
fi
if [[ -f /etc/bot/config.testnet.toml ]]; then
  sed -i 's|edge_profile_path = "config/edge_profile.toml"|edge_profile_path = "/etc/bot/edge_profile.toml"|' /etc/bot/config.testnet.toml || true
fi

if [[ ! -f /etc/bot/secrets.env ]]; then
  if [[ -f "$SRC/etc/secrets.env.example" ]]; then
    install -m 600 -o bot -g bot "$SRC/etc/secrets.env.example" /etc/bot/secrets.env
  elif [[ -f "$SRC/secrets.env.example" ]]; then
    install -m 600 -o bot -g bot "$SRC/secrets.env.example" /etc/bot/secrets.env
  fi
fi

patch_zenoh_peer() {
  local ip="$1"
  local f=/etc/bot/zenoh.json5
  if [[ -n "$ip" && -f "$f" ]]; then
    sed -i "s/TOKYO_PRIVATE_IP/${ip}/g" "$f"
    sed -i "s/SINGAPORE_PRIVATE_IP/${ip}/g" "$f"
  fi
}

if [[ "$ROLE" == "tokyo" ]]; then
  if [[ ! -f /etc/bot/zenoh.json5 ]]; then
    install -m 644 "$SRC/etc/zenoh-tokyo.json5.example" /etc/bot/zenoh.json5
  fi
  if [[ -n "$PEER_IP" ]]; then
    # optional dial back to Singapore
    if ! grep -q "$PEER_IP" /etc/bot/zenoh.json5 2>/dev/null; then
      sed -i "s|endpoints: \[\]|endpoints: [\"tcp/${PEER_IP}:7447\"]|" /etc/bot/zenoh.json5 || true
    fi
  fi
  install -m 644 "$SRC/systemd/observer.service" /etc/systemd/system/observer.service
  systemctl daemon-reload
  systemctl enable observer
  systemctl restart observer || systemctl start observer
  echo "TOKYO_OK observer started"
elif [[ "$ROLE" == "singapore" ]]; then
  if [[ ! -f /etc/bot/zenoh.json5 ]]; then
    install -m 644 "$SRC/etc/zenoh-singapore.json5.example" /etc/bot/zenoh.json5
  fi
  if [[ -z "$PEER_IP" ]]; then
    echo "WARN: set PEER_IP=<tokyo_private_ip> so Zenoh can connect" >&2
  else
    patch_zenoh_peer "$PEER_IP"
  fi
  for u in executor control-panel telegram-alerts; do
    install -m 644 "$SRC/systemd/${u}.service" /etc/systemd/system/${u}.service
  done
  systemctl daemon-reload
  systemctl enable executor control-panel telegram-alerts
  systemctl restart executor control-panel telegram-alerts || systemctl start executor control-panel telegram-alerts
  echo "SINGAPORE_OK executor stack started (PEER_IP=${PEER_IP:-unset})"
fi

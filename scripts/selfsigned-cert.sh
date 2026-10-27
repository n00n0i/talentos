#!/usr/bin/env bash
# Self-signed certificate for on-prem testing. Replace ./certs with real
# certificates (Let's Encrypt, internal CA) before going live.
set -euo pipefail
cd "$(dirname "$0")/.."

HOST="${1:-localhost}"
mkdir -p certs
openssl req -x509 -nodes -newkey rsa:2048 -days 365 \
    -keyout certs/privkey.pem -out certs/fullchain.pem \
    -subj "/CN=$HOST" \
    -addext "subjectAltName=DNS:$HOST,DNS:localhost,IP:127.0.0.1" 2>/dev/null
chmod 600 certs/privkey.pem
echo "self-signed cert for $HOST written to ./certs (valid 365 days)"

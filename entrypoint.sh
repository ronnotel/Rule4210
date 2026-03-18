#!/bin/sh
set -e

# Start the Rule 4210 server in background, piping output to container stdout
echo "Starting server from $(pwd), binary exists: $(ls -la /app/server 2>&1)"
/app/server 2>&1 &
echo "server PID: $!"

# If a Cloudflare tunnel token is provided, start cloudflared
if [ -n "$TUNNEL_TOKEN" ]; then
    exec cloudflared tunnel --no-autoupdate run --token "$TUNNEL_TOKEN"
else
    # No tunnel — just wait for the server process
    wait
fi

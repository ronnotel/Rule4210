#!/bin/sh

echo "binary: $(ls -la /app/server 2>&1)"

# Auto-restart loop for the server
(while true; do
    echo "Starting Rule 4210 server..."
    /app/server 2>&1
    echo "Server exited ($?), restarting in 1s..."
    sleep 1
done) &

# Start cloudflared (or wait if no token)
if [ -n "$TUNNEL_TOKEN" ]; then
    exec cloudflared tunnel --no-autoupdate run --token "$TUNNEL_TOKEN"
else
    wait
fi

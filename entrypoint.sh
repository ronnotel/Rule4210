#!/bin/sh

echo "binary: $(ls -la /app/server 2>&1)"

# Auto-restart loop for the Rust server
(while true; do
    echo "Starting Rule 4210 server..."
    /app/server 2>&1
    echo "Server exited ($?), restarting in 1s..."
    sleep 1
done) &

# Auto-restart loop for cloudflared
if [ -n "$TUNNEL_TOKEN" ]; then
    while true; do
        echo "Starting cloudflared..."
        cloudflared tunnel --no-autoupdate --protocol http2 run --token "$TUNNEL_TOKEN"
        echo "cloudflared exited ($?), restarting in 3s..."
        sleep 3
    done
else
    wait
fi

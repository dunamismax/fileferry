# Homepage Deployment

`fileferry.app` is served by `fileferry-web`, a lightweight Rust binary that uses
Axum and server-rendered Leptos views. This binary is public marketing
infrastructure only; it is not a FileFerry backup server, daemon, scheduler, or
hosted product.

## Local Run

```sh
cargo run -p fileferry-web
```

The default listener is `0.0.0.0:8080`. Override it with:

```sh
FILEFERRY_WEB_ADDR=127.0.0.1:8096 cargo run -p fileferry-web
```

`GET /healthz` returns `ok` for reverse-proxy and process supervision checks.

## Ubuntu Shape

Build the binary on the server or copy a release artifact once release
packaging exists:

```sh
cargo build --release -p fileferry-web
sudo install -m 0755 target/release/fileferry-web /usr/local/bin/fileferry-web
```

The production host uses the repo-owned templates under `deploy/` and binds
the homepage to `127.0.0.1:8096` so it does not conflict with other local Rust
sites.

Create a dedicated unprivileged user:

```sh
sudo useradd --system --home /opt/fileferry-web --shell /usr/sbin/nologin fileferry-web
```

Example systemd unit:

```ini
[Unit]
Description=FileFerry public homepage
After=network-online.target
Wants=network-online.target

[Service]
User=fileferry-web
Group=fileferry-web
Environment=FILEFERRY_WEB_ADDR=127.0.0.1:8096
ExecStart=/opt/fileferry-web/fileferry-web
Restart=on-failure
RestartSec=5s
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/fileferry-web

[Install]
WantedBy=multi-user.target
```

Install and start it:

```sh
sudo install -m 0644 deploy/systemd/fileferry-web.service /etc/systemd/system/fileferry-web.service
sudo systemctl daemon-reload
sudo systemctl enable --now fileferry-web
curl -fsS http://127.0.0.1:8096/healthz
```

## Reverse Proxy

Terminate TLS at the reverse proxy and forward to the local listener.

Example Caddy site:

```caddyfile
fileferry.app {
	reverse_proxy 127.0.0.1:8096
}
```

Example nginx server:

```nginx
server {
    listen 80;
    server_name fileferry.app www.fileferry.app;

    location / {
        proxy_pass http://127.0.0.1:8096;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Use the proxy's normal TLS automation or certificate deployment path before
serving production traffic.

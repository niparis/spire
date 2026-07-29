# Linear webhook ingress

Sprint 06 exposes only `POST /webhooks/linear` over Cloudflare Tunnel. The
Spire API and admin listener bind to loopback; no inbound firewall port is
opened on the VM.

## Install and verify

1. Copy `deploy/systemd/spire.service` and `deploy/systemd/cloudflared.service`
   to `/etc/systemd/system/`; install the tunnel configuration at
   `/etc/cloudflared/config.yml`.
2. Replace every `REPLACE_ME` value, provision the two Spire systemd
   credentials, and validate the application configuration with
   `spire config validate --config /etc/spire/spire.yaml`.
3. Configure the remotely managed tunnel DNS hostname to the exact webhook
   path. Do not add an ingress rule for `/admin` or a catch-all origin rule.
4. Run `systemctl daemon-reload`, `systemctl enable --now spire cloudflared`,
   then check `curl --fail http://127.0.0.1:8080/health/ready` locally.
5. Register the public HTTPS webhook URL in Linear with its configured webhook
   ID, organization ID, and signing secret. A valid signed delivery returns
   `200` only after the inbox insert commits; duplicate deliveries also return
   `200`.

## Rotation, health, and removal

- Rotate the Linear signing secret by updating its systemd credential and the
  Linear webhook secret, then restart `spire`. Do not log or place the secret
  in YAML.
- Check `systemctl status spire cloudflared` and both loopback health paths.
  A tunnel restart does not affect SQLite state; reconciliation repairs events
  missed beyond Linear's retry window.
- To remove ingress, disable the Cloudflare public hostname first, then stop
  and disable `cloudflared`. Keep Spire running long enough for recovery and
  reconciliation, or stop both services intentionally.

## Limits and operator-owned values

The tunnel ID, hostname, Cloudflare Access policy, Linear webhook ID, and
credentials are intentionally not committed. The operator must supply them;
the service fails closed when placeholders or missing credentials are used.

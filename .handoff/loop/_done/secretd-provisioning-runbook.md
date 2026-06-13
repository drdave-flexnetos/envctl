# secretd vault provisioning runbook — BLOCKED on hardware (dedicated USB token)

**Status:** BLOCKED — requires the user's **dedicated spare USB stick** (unlock posture **U1**,
honors THREAT-MODEL FS-S22). This is the human/hardware step AFTER the `fix-secretd` build. Do NOT
run the `--apply` steps until the USB token is inserted.

## What is already done (the `fix-secretd` build, GREEN)
- `secretd` sends `sd_notify(READY=1)` → `Type=notify` no longer times out (crash loop fixed).
- Durable store wired: a loopback `sqld` (libSQL remote) on `http://127.0.0.1:8080`; secretd reads
  `~/.config/env-ctl/secretd.toml` `[store] backend="libsql"`.
- `Vault.Init` RPC + `secretctl init` verb exist (apply-gated, owner-only, forced Argon2 floor,
  refuses re-init).
- The stack is **HEALTHY**: `systemctl --user is-active env-ctl.service` → `active (running)`,
  `secretctl status` → `locked` (healthy; vault unprovisioned).

## Why it is blocked
1. **USB enrollment needs the dedicated USB stick.** `secretctl init --enroll-usb --usb-partuuid
   <UUID> --apply` reads the keyfile from the USB partition via the daemon's `UsbProbe` seam. The
   shipped `RealUsbProbe::keyfile_for` is an **unimplemented hardware seam** in this build, so an
   `--enroll-usb --apply` returns a clean refusal (it does NOT panic) until that seam is wired to
   the real USB partition + the dedicated stick is present. Building that hardware probe + inserting
   the stick is the provisioning step.
2. USB **unlock** uses the same seam, so unlock-via-USB is gated on the same hardware step.

## Provisioning steps (run ONLY with the dedicated USB token inserted)

> Prereq each new shell: `export SECRETCTL_SOCK="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/env-ctl/secretd.sock"`

1. **Insert the dedicated USB token** (the spare stick — NOT the Ubuntu installer).
2. **Identify its partition UUID:**
   ```sh
   lsblk -o NAME,PARTUUID,UUID,SIZE,MOUNTPOINT
   # note the PARTUUID of the keyfile partition (the slot selector; not the key)
   ```
3. **Place the keyfile on the USB partition** at the path the `UsbProbe` impl expects (per
   ARCHITECTURE / THREAT-MODEL FS-S9 — possession is proven cryptographically, UUID match alone is
   not possession).
4. **Initialize the vault (USB + passphrase), apply:**
   ```sh
   printf '%s' "<strong-passphrase>" | \
     secretctl init --passphrase-stdin --enroll-usb --usb-partuuid <PARTUUID> --apply
   ```
   - Dry-run first (drop `--apply`) to preview; `init` REFUSES to overwrite an existing vault.
   - Argon2 is forced server-side to the hardened floor (m=1 GiB, t=4, p=4); the client never
     supplies KDF params.
5. **Unlock the vault:**
   ```sh
   secretctl unlock                       # USB-first (token must be inserted)
   # or, if USB absent:  secretctl unlock --passphrase-stdin
   secretctl status                       # expect: unlocked
   ```
6. **Store the n8n key:**
   ```sh
   printf '%s' "<n8n-api-key>" | \
     secretctl secret add n8n-api-key --provider generic --value-stdin --broker-only
   ```

## Verification after provisioning
- `secretctl status` → `unlocked` with the expected `secret_count`.
- `systemctl --user is-active env-ctl.service` stays `active (running)` across a restart, and the
  vault survives the restart (durable libSQL store) — re-`unlock` after a restart, secrets persist.

#!/usr/bin/env bash
#
# ubuntu-boot-repair.sh — guarded UEFI boot repair / bootloader-id rename
# Ubuntu 26.04 (GRUB 2.14, dracut 110 initramfs backend)
#
#   nvme0n1 (broken)  -> repaired & registered as  ubuntu26dev
#   nvme1n1 (running) -> re-registered as           ubuntu26pro
#
# HARDENED after adversarial review (findings C1-C5,H1-H4,M1,M5,M6).
#
# SAFETY MODEL
#   * Targets resolved BY UUID only (no label fallback); the resolved
#     device's UUID is RE-VERIFIED before any mount.
#   * BOTH root and ESP are guarded: must match expected UUID, must NOT
#     be the live device, and must live on the SAME physical disk.
#   * Refuses to operate if the dev root UUID == the live root UUID.
#   * The dev /home (425GB data) is NEVER mounted, unmounted, or named in
#     any operation. If it is currently mounted, we abort and ask you to
#     handle it — we never run umount on it.
#   * Default action is read-only diagnosis. Writes only under an explicit
#     subcommand.
#   * initramfs is rebuilt ONLY if missing/too-small, is BACKED UP first,
#     and is built GENERIC (--no-hostonly) so a chroot rebuild is safe.
#   * After repair, asserts grub.cfg references the DEV root UUID and NOT
#     the live root UUID, and that the NVRAM entry was created.
#   * Nothing is ever deleted. Retiring the stale "ubuntu" entry/dir is
#     left to you, by hand, AFTER a confirmed reboot (see FINAL STEPS).
#   * sync + plain-umount(then lazy) on cleanup, via an EXIT trap.
#
# USAGE (run as root, on the working nvme1n1 system)
#   sudo bash ubuntu-boot-repair.sh diagnose     # read-only: show dev /boot state
#   sudo bash ubuntu-boot-repair.sh repair-dev   # fix nvme0n1 -> ubuntu26dev
#   sudo bash ubuntu-boot-repair.sh rename-pro   # rename live nvme1n1 -> ubuntu26pro
#
set -euo pipefail

# ---- expected identities (verified true on 2026-06-01) ---------------------
DEV_ROOT_UUID="7f8c16c8-b9a2-4fb2-87fe-4e530db6ae6d"   # nvme0n1p2  ext4  (broken root)
DEV_ESP_UUID="4CB2-1FFC"                                # nvme0n1p1  vfat  (broken ESP)
DEV_HOME_UUID="5d9eeffb-cf79-4483-b97d-33a0140154f6"   # nvme0n1p3  ext4  (DATA - never touch)
LIVE_ROOT_UUID="8f9bd935-41d2-4275-837c-88d4bca13d85"  # nvme1n1p2  (sanity cross-check)
DEV_BL_ID="ubuntu26dev"
PRO_BL_ID="ubuntu26pro"

MNT=""        # set when we mount; used by cleanup trap
KVER=""       # detected from the dev disk, not hardcoded (fix M5)

die(){ echo "ABORT: $*" >&2; exit 1; }
note(){ echo ">> $*"; }

require_root(){ [ "$(id -u)" -eq 0 ] || die "must run as root (sudo)"; }
require_uefi(){ [ -d /sys/firmware/efi ] || die "not booted in UEFI mode — do not proceed"; }
have(){ command -v "$1" >/dev/null 2>&1 || die "required tool missing: $1"; }

# Resolve a device from a UUID, by UUID ONLY. No label fallback (fix C1).
dev_from_uuid(){ blkid -U "$1" 2>/dev/null || true; }
uuid_of(){ blkid -s UUID -o value "$1" 2>/dev/null || true; }
disk_of(){ lsblk -no PKNAME "$1" 2>/dev/null | head -1; }

cleanup(){
  set +e
  [ -n "$MNT" ] || return 0
  sync                                  # flush before detaching (fix M1)
  for d in run sys/firmware/efi/efivars sys proc dev/pts dev boot/efi; do
    if mountpoint -q "$MNT/$d"; then umount "$MNT/$d" 2>/dev/null || umount -l "$MNT/$d"; fi
  done
  if mountpoint -q "$MNT"; then umount "$MNT" 2>/dev/null || umount -l "$MNT"; fi
  rmdir "$MNT" 2>/dev/null
}
trap cleanup EXIT

# Resolve + RE-VERIFY a device by UUID; abort on any mismatch (fixes C1,C2).
resolve_verified(){   # $1=uuid  -> echoes device path or dies
  local want="$1" dev
  dev="$(dev_from_uuid "$want")"
  [ -n "$dev" ] || die "UUID $want not found — is the disk attached / udev settled?"
  [ "$(uuid_of "$dev")" = "$want" ] || die "resolved $dev does not actually carry UUID $want"
  echo "$dev"
}

mount_dev_root(){   # arg: ro|rw
  local mode="$1" root_dev esp_dev live_root live_esp rdisk edisk
  have blkid; have lsblk; have findmnt

  root_dev="$(resolve_verified "$DEV_ROOT_UUID")"
  esp_dev="$(resolve_verified "$DEV_ESP_UUID")"

  # --- guards: never the live system (fixes C2) ---
  live_root="$(findmnt -no SOURCE /)"
  live_esp="$(findmnt -no SOURCE /boot/efi 2>/dev/null || true)"
  [ "$(uuid_of "$live_root")" != "$DEV_ROOT_UUID" ] || die "dev root UUID == LIVE root — refusing"
  [ "$root_dev" != "$live_root" ] || die "dev root device == live root device — refusing"
  [ -z "$live_esp" ] || [ "$esp_dev" != "$live_esp" ] || die "dev ESP == live ESP — refusing"

  # --- guard: root and ESP must be on the SAME physical disk (fix C2) ---
  rdisk="$(disk_of "$root_dev")"; edisk="$(disk_of "$esp_dev")"
  [ -n "$rdisk" ] && [ "$rdisk" = "$edisk" ] || die "dev root ($rdisk) and ESP ($edisk) are on different disks — refusing"

  # --- guard: the DATA /home must not be mounted; we NEVER umount it (fix C3, F1) ---
  # Check both by resolved device AND by UUID (catches by-uuid/mapper/bind mounts).
  local home_dev; home_dev="$(dev_from_uuid "$DEV_HOME_UUID")"
  if { [ -n "$home_dev" ] && findmnt -S "$home_dev" >/dev/null 2>&1; } \
     || findmnt --source "UUID=$DEV_HOME_UUID" >/dev/null 2>&1; then
    die "dev /home (UUID $DEV_HOME_UUID) is currently mounted. Unmount it yourself, then re-run. (This script will not touch it.)"
  fi

  # release only ROOT/ESP auto-mounts (verified devices), never home
  for d in "$root_dev" "$esp_dev"; do
    if findmnt -S "$d" >/dev/null 2>&1; then note "releasing auto-mount of $d"; umount "$d" || umount -l "$d"; fi
  done

  MNT="$(mktemp -d /tmp/devroot.XXXXXX)"
  note "mounting dev root $root_dev ($mode) at $MNT"
  mount -o "$mode" "$root_dev" "$MNT"

  if [ "$mode" = "rw" ]; then
    [ -d "$MNT/boot/efi" ] || mkdir -p "$MNT/boot/efi"          # rw only (fix M6)
  fi
  if [ -d "$MNT/boot/efi" ]; then
    note "mounting dev ESP $esp_dev at $MNT/boot/efi"
    mount -o "$mode" "$esp_dev" "$MNT/boot/efi"
  else
    note "WARN: $MNT/boot/efi missing on a read-only mount — skipping ESP mount"
  fi
  note "NOTE: dev /home ($DEV_HOME_UUID) intentionally NOT mounted"
}

# Detect the dev disk's newest installed kernel version (fix M5).
detect_kver(){
  local k
  k="$(ls -1 "$MNT"/boot/vmlinuz-*-generic 2>/dev/null | sort -V | tail -1 || true)"
  if [ -n "$k" ]; then basename "$k" | sed 's/^vmlinuz-//'; return; fi
  # fall back to dpkg record on the dev disk
  grep -oE '^Package: linux-image-[0-9][^ ]*' "$MNT/var/lib/dpkg/status" 2>/dev/null \
    | sed 's/^Package: linux-image-//' | sort -V | tail -1
}

diagnose(){
  require_root; require_uefi
  mount_dev_root ro
  KVER="$(detect_kver)"; note "detected dev kernel: ${KVER:-<none found>}"
  echo "================= DEV DISK /boot DIAGNOSIS (read-only) ================="
  echo "--- ESP layout ---";              ls -la "$MNT/boot/efi/EFI" 2>&1 || true
  echo "--- kernel + initrd on root ---";  ls -la "$MNT"/boot/vmlinuz-* "$MNT"/boot/initrd.img-* 2>&1 || true
  echo "--- real grub.cfg present? ---";   ls -la "$MNT/boot/grub/grub.cfg" 2>&1 || true
  if [ -n "$KVER" ]; then
    echo "--- initrd for $KVER size (bytes) ---"; stat -c%s "$MNT/boot/initrd.img-$KVER" 2>&1 || true
    echo "--- kernel pkg state ---"; grep -A1 "^Package: linux-image-$KVER\$" "$MNT/var/lib/dpkg/status" 2>&1 | head -2 || true
  fi
  echo "--- fstab UUIDs (should match dev disk) ---"; grep -vE '^\s*#|^\s*$' "$MNT/etc/fstab" 2>&1 || true
  echo "======================================================================="
  echo "Interpretation:"
  echo "  * initrd present & >~10MB        -> DO NOT rebuild; repair-dev will preserve it"
  echo "  * initrd missing/tiny            -> repair-dev rebuilds it GENERIC (--no-hostonly) after backup"
  echo "  * /boot/grub/grub.cfg missing    -> repair-dev regenerates it"
  echo "  * vmlinuz-$KVER missing          -> kernel pkg reinstall needed (requires apt/network in chroot)"
}

repair_dev(){
  require_root; require_uefi
  have grub-install; have efibootmgr
  mount_dev_root rw
  KVER="$(detect_kver)"; [ -n "$KVER" ] || die "could not detect a kernel on the dev disk (vmlinuz missing?) — resolve manually"
  # F2: the detected kernel's image must actually exist on disk, else grub.cfg would
  # reference a non-existent kernel and the dev disk stays unbootable.
  [ -s "$MNT/boot/vmlinuz-$KVER" ] || die "kernel image /boot/vmlinuz-$KVER missing on dev disk — reinstall the kernel package first (needs apt/network)"
  note "target kernel: $KVER (vmlinuz present)"

  # research rec (finding D): snapshot NVRAM before so we can confirm non-clobber
  NVRAM_BEFORE="$(efibootmgr | sed 's/^/   /')"
  note "NVRAM entries BEFORE repair (for non-clobber comparison):"; echo "$NVRAM_BEFORE"

  note "binding pseudo-filesystems into chroot (incl. efivars rw for NVRAM) — fix H2"
  for d in /dev /dev/pts /proc /sys; do mount --bind "$d" "$MNT$d"; done
  mount --bind /run "$MNT/run"
  if [ -d /sys/firmware/efi/efivars ]; then
    mount --bind /sys/firmware/efi/efivars "$MNT/sys/firmware/efi/efivars" 2>/dev/null || true
  fi

  note "entering chroot — conditional initramfs, grub-install '$DEV_BL_ID', update-grub"
  chroot "$MNT" /usr/bin/env KVER="$KVER" BL="$DEV_BL_ID" DEVROOT="$DEV_ROOT_UUID" LIVEROOT="$LIVE_ROOT_UUID" \
    /bin/bash -euo pipefail <<'CHROOT'
    set -euo pipefail
    initrd="/boot/initrd.img-$KVER"

    # --- initramfs: rebuild ONLY if missing/too small; back up; GENERIC (fixes C4,C5,H1) ---
    needs_initrd=0
    if [ ! -s "$initrd" ]; then needs_initrd=1
    elif [ "$(stat -c%s "$initrd")" -lt 10000000 ]; then needs_initrd=1; fi
    if [ "$needs_initrd" -eq 1 ]; then
      echo ">> [chroot] initrd missing/too small — backing up and rebuilding GENERIC"
      [ -e "$initrd" ] && cp -a "$initrd" "$initrd.bak.$(date +%s 2>/dev/null || echo bak)" || true
      if command -v dracut >/dev/null 2>&1; then
        dracut --no-hostonly --force "$initrd" "$KVER"
      else
        update-initramfs -c -k "$KVER"
      fi
      [ -s "$initrd" ] && [ "$(stat -c%s "$initrd")" -ge 10000000 ] || { echo "!! initrd rebuild failed/too small"; exit 1; }
    else
      echo ">> [chroot] initrd present and healthy ($(stat -c%s "$initrd") bytes) — PRESERVING it"
    fi

    # --- bootloader: install under the dev id; no host bleed from os-prober (fix H4) ---
    echo ">> [chroot] grub-install as $BL"
    grub-install --target=x86_64-efi --efi-directory=/boot/efi --bootloader-id="$BL" --recheck
    echo ">> [chroot] disabling os-prober persistently (drop-in) — env var alone is overridden by /etc/default/grub"
    mkdir -p /etc/default/grub.d
    printf 'GRUB_DISABLE_OS_PROBER=true\n' > /etc/default/grub.d/99-no-osprober.cfg
    echo ">> [chroot] update-grub"
    update-grub

    # --- assert grub.cfg points at the DEV root, not the live root (fix H4) ---
    grep -q "$DEVROOT" /boot/grub/grub.cfg || { echo "!! grub.cfg does NOT reference dev root $DEVROOT"; exit 1; }
    if grep -q "$LIVEROOT" /boot/grub/grub.cfg; then echo "!! grub.cfg references the LIVE root $LIVEROOT — host bleed"; exit 1; fi
    echo ">> [chroot] grub.cfg OK: references dev root, not live root"
    grep -E "linux\s|initrd\s" /boot/grub/grub.cfg | head -8 || true
CHROOT

  sync
  note "back on host — verifying NVRAM entry exists (fix H2)"
  efibootmgr -v | grep -i "$DEV_BL_ID" \
    || die "no '$DEV_BL_ID' NVRAM entry was created — grub-install could not write efivars from chroot; resolve before rebooting"
  note "NVRAM entries AFTER repair (compare against BEFORE — only '$DEV_BL_ID' should be new):"
  efibootmgr | sed 's/^/   /'
  echo
  echo "repair-dev complete and verified. The dev disk's /home was never mounted."
}

rename_pro(){
  require_root; require_uefi
  have grub-install; have efibootmgr
  local live_dev live_uuid
  live_dev="$(findmnt -no SOURCE /)"
  live_uuid="$(uuid_of "$live_dev")"
  [ "$live_uuid" = "$LIVE_ROOT_UUID" ] || die "live root UUID ($live_uuid) != expected pro UUID ($LIVE_ROOT_UUID) — refusing"
  mountpoint -q /boot/efi || die "/boot/efi is not mounted on the live system — refusing"
  note "renaming LIVE system ($live_dev) bootloader id to '$PRO_BL_ID'"
  note "the existing 'ubuntu' entry stays valid until you remove it by hand"
  grub-install --target=x86_64-efi --efi-directory=/boot/efi --bootloader-id="$PRO_BL_ID" --recheck
  note "disabling os-prober persistently (drop-in) for a clean per-disk menu"
  mkdir -p /etc/default/grub.d
  printf 'GRUB_DISABLE_OS_PROBER=true\n' > /etc/default/grub.d/99-no-osprober.cfg
  update-grub
  echo ">> NVRAM entries now:"; efibootmgr -v | grep -iE "$PRO_BL_ID|ubuntu" || true
  efibootmgr -v | grep -iq "$PRO_BL_ID" || die "no '$PRO_BL_ID' NVRAM entry created"
  echo
  echo "rename-pro complete. Reboot via '$PRO_BL_ID' and confirm BEFORE deleting the old 'ubuntu' entry/dir."
}

finalize_nvram(){
  # Create TWO distinctly-labeled NVRAM entries so they can never de-dup each
  # other. Pro is the running system — we DO NOT grub-install it; we just point
  # a named entry at its existing intact \EFI\ubuntu shim. Nothing is deleted.
  require_root; require_uefi
  have efibootmgr; have lsblk; have findmnt

  local dev_esp pro_esp dev_disk pro_disk dev_pno pro_pno
  dev_esp="$(resolve_verified "$DEV_ESP_UUID")"          # nvme0n1p1
  pro_esp="$(findmnt -no SOURCE /boot/efi)"               # live pro ESP
  [ -n "$pro_esp" ] || die "/boot/efi not mounted on the live system"
  [ "$(uuid_of "$pro_esp")" != "$DEV_ESP_UUID" ] || die "live /boot/efi resolves to the DEV esp — refusing"
  dev_disk="/dev/$(disk_of "$dev_esp")"; dev_pno="$(echo "$dev_esp" | grep -oE '[0-9]+$')"
  pro_disk="/dev/$(disk_of "$pro_esp")"; pro_pno="$(echo "$pro_esp" | grep -oE '[0-9]+$')"
  note "dev ESP: $dev_esp -> $dev_disk part $dev_pno"
  note "pro ESP: $pro_esp -> $pro_disk part $pro_pno"

  # verify the pro shim exists (live mount) and the dev shim exists (ro mount)
  [ -f /boot/efi/EFI/ubuntu/shimx64.efi ] || die "pro shim missing at \\EFI\\ubuntu\\shimx64.efi"
  mount_dev_root ro
  [ -f "$MNT/boot/efi/EFI/ubuntu26dev/shimx64.efi" ] || die "dev shim missing at \\EFI\\ubuntu26dev — run repair-dev first"
  note "both shim binaries present — creating distinct NVRAM entries (idempotent)"

  echo ">> NVRAM BEFORE:"; efibootmgr | sed 's/^/   /'
  if ! efibootmgr | grep -qw "ubuntu26dev"; then
    efibootmgr -c -d "$dev_disk" -p "$dev_pno" -L "ubuntu26dev" -l '\EFI\ubuntu26dev\shimx64.efi' >/dev/null
    note "created ubuntu26dev"
  else note "ubuntu26dev already present — leaving as-is"; fi
  if ! efibootmgr | grep -qw "ubuntu26pro"; then
    efibootmgr -c -d "$pro_disk" -p "$pro_pno" -L "ubuntu26pro" -l '\EFI\ubuntu\shimx64.efi' >/dev/null
    note "created ubuntu26pro"
  else note "ubuntu26pro already present — leaving as-is"; fi

  echo ">> NVRAM AFTER (verify both ubuntu26dev and ubuntu26pro point at the right ESP):"
  efibootmgr -v | grep -iE "ubuntu26dev|ubuntu26pro" || true
  echo
  efibootmgr | grep -qw "ubuntu26dev" || die "ubuntu26dev entry not created"
  efibootmgr | grep -qw "ubuntu26pro" || die "ubuntu26pro entry not created"
  echo "finalize complete. NOTHING was deleted. Set boot order with, e.g.:"
  echo "   sudo efibootmgr -o <ubuntu26pro#>,<ubuntu26dev#>   # numbers from 'efibootmgr' above"
  echo "Then reboot and test BOTH entries before removing any old 'Ubuntu' entry."
}

case "${1:-}" in
  diagnose)      diagnose ;;
  repair-dev)    repair_dev ;;
  rename-pro)    rename_pro ;;
  finalize)      finalize_nvram ;;
  *) echo "usage: sudo bash $0 {diagnose|repair-dev|rename-pro|finalize}"; exit 2 ;;
esac

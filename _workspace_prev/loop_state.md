# Loop state — env-install-loop
session_started: 2026-06-05T02:52:37Z
loop: env-install-loop
branch: env-install-loop
worktree: /home/drdave/Desktop/meta/.worktrees/env-install-loop/envctl
cycle_budget: 3
cycles_this_session: 2
cycles_total: 18
last_item: gpu-verify-scripts (SIGPIPE gate fix) + group-gpu-stack (detect-by-path) + re-lock
status: |
  DONE. 45/46 declared components detected + healthy; zero drift. Only node-via-bun remains
  undetected — TABLED by design (n8n needs real node; bun can't be n8n's node; reset refused
  by fail-closed group-ai-clis reverse-dep guard).
  This session (resumed fresh): merged origin/master (gpu-verify port #17 + auto-provision #15),
  then closed the two previously by-design GPU gaps as REAL loop work:
  (1) gpu-verify-scripts — fixed a SIGPIPE/pipefail bug in the shipped NVIDIA gate
      (lspci | grep -q under pipefail → false "no GPU"); redeployed; install+verify GREEN.
  (2) group-gpu-stack — detect now resolves nvcc by installed path (cuda-toolkit's verify
      pattern), independent of the non-interactive-shell PATH accident.
  doctor green; lock --check clean (46); kasetto sync --locked clean; build + no-c/shape/enable
  PASS; gpu smoke test green (2x RTX 5090, torch sm_120, cargo-oxide, Podman CDI).
needs_human_followups: |
  - node-via-bun: manifest design — mark not-applicable when real Node in n8n range present
    (or add node-real component + drop the group-ai-clis -> node-via-bun edge).
  - CUDA non-interactive PATH: envctl wires cuda env into ~/.bashrc after the interactivity
    guard, so nvcc isn't on PATH for non-interactive shells/systemd. Feature-Forge could wire
    it system-wide (/etc/profile.d/cuda.sh). Detect is now truthful regardless.
last_update: 2026-06-05T (resumed session)

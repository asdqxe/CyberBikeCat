#!/usr/bin/env bash
# Bounded two-process IPC check. No fullscreen terminal or real agent.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
pet=./target/release/avcii-sketch
event_dir=$(mktemp -d "$PWD/.pet-runtime-check.XXXXXX")
viewer_pid=
finish() {
    if [[ -n "$viewer_pid" ]]; then
        kill "$viewer_pid" 2>/dev/null || true
        wait "$viewer_pid" 2>/dev/null || true
    fi
}
trap finish EXIT
timeout 13s "$pet" view "$event_dir" --headless 10 > "$event_dir/states.log" 2>&1 &
viewer_pid=$!
for attempt in {1..40}; do
    [[ -S "$event_dir/events.sock" ]] && break
    sleep 0.05
done
if [[ ! -S "$event_dir/events.sock" ]]; then
    cat "$event_dir/states.log" >&2
    exit 1
fi
[[ $(stat -c %a "$event_dir") == 700 ]]
[[ $(stat -c %a "$event_dir/events.sock") == 600 ]]
reject() {
    local result=0
    timeout 2s "$@" > "$event_dir/rejected.log" 2>&1 || result=$?
    if [[ "$result" -ne 1 ]]; then
        printf 'Expected exit 1, received %s\n' "$result" >&2
        exit 1
    fi
}
reject "$pet" view "$event_dir" --headless 1
[[ -S "$event_dir/events.sock" ]] # second viewer must not remove the first socket
reject "$pet" emit "$event_dir" real busy
reject "$pet" emit "$event_dir" fake unknown
timeout 2s "$pet" emit "$event_dir" fake busy
sleep 0.2
timeout 2s "$pet" say "$event_dir" 提速
sleep 0.1
timeout 2s "$pet" say "$event_dir" 向左闪避
sleep 0.2
timeout 2s "$pet" say "$event_dir" 向右闪避
sleep 0.2
timeout 2s "$pet" say "$event_dir" 回中
sleep 0.1
reject "$pet" say "$event_dir" 不要向左
timeout 2s "$pet" emit "$event_dir" fake done
sleep 2.3
timeout 2s "$pet" emit "$event_dir" fake active
sleep 0.2
timeout 2s "$pet" emit "$event_dir" fake error
wait "$viewer_pid"
viewer_pid=
for state in BUSY DONE IDLE ACTIVE ERROR DISCONNECTED; do
    grep -q "source=FAKE state=$state" "$event_dir/states.log"
done
for action in 'turn=LEFT' 'turn=RIGHT' 'turn=CENTER' 'boost=true'; do
    grep -q "source=FAKE state=BUSY.*$action" "$event_dir/states.log"
done
[[ ! -e "$event_dir/events.sock" ]]
reject "$pet" emit "$event_dir" fake busy
cat "$event_dir/states.log"
printf 'PASS: text actions preserve BUSY, state transitions, expiry, permissions, duplicate viewer, rejection and socket cleanup.\nLogs: %s\n' "$event_dir"

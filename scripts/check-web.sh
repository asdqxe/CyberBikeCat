#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
log_dir=$(mktemp -d "$PWD/.pet-runtime-web.XXXXXX")
server_pid=
finish() {
    if [[ -n "$server_pid" ]]; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
    fi
}
trap finish EXIT
timeout 13s ./target/release/avcii-sketch web 0 --seconds 10 > "$log_dir/server.log" 2>&1 &
server_pid=$!
url=
for attempt in {1..40}; do
    url=$(awk '/^Open / {print $2}' "$log_dir/server.log")
    [[ -n "$url" ]] && break
    sleep 0.05
done
if [[ -z "$url" ]]; then cat "$log_dir/server.log" >&2; exit 1; fi
request() { curl --noproxy 127.0.0.1 --max-time 2 -sS "$@"; }
request --fail "$url/" > "$log_dir/index.html"
grep -q '提交预览' "$log_dir/index.html"
request --fail "$url/chat-feedback.js" > "$log_dir/chat-feedback.js"
grep -q 'createChatFeedback' "$log_dir/chat-feedback.js"
for state in busy done error cancelled; do
    request --fail -H "Origin: $url" -H 'Content-Type: text/plain;charset=UTF-8' --data "preview $state" "$url/event" > /dev/null
    sleep 0.1
    request --fail "$url/frame" > "$log_dir/frame.json"
    grep -q '"source":"PREVIEW"' "$log_dir/frame.json"
    grep -q "\"state\":\"${state^^}\"" "$log_dir/frame.json"
done
code=$(request -o /dev/null -w '%{http_code}' -H 'Origin: https://example.invalid' -H 'Content-Type: text/plain;charset=UTF-8' --data 'chat busy' "$url/event")
[[ "$code" == 403 ]]
code=$(request -o /dev/null -w '%{http_code}' -H "Origin: $url" -H 'Content-Type: text/plain;charset=UTF-8' --data 'preview unknown' "$url/event")
[[ "$code" == 400 ]]
code=$(request -o /dev/null -w '%{http_code}' "$url/../../Cargo.toml")
[[ "$code" == 404 ]]
wait "$server_pid"
server_pid=
printf 'PASS: embedded assets, preview lifecycle, origin rejection, invalid events, no file serving, finite shutdown.\nLogs: %s\n' "$log_dir"

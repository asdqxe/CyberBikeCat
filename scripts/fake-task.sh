#!/usr/bin/env bash
# Finite local event producer; does not launch or observe an agent.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
event_dir=${1:-.pet-runtime}
outcome=${2:-ok}
if [[ $# -gt 2 || ( "$outcome" != ok && "$outcome" != fail ) ]]; then
    printf 'Usage: bash scripts/fake-task.sh [EVENT_DIR] [ok|fail]\n' >&2
    exit 2
fi
pet=./target/release/avcii-sketch
if [[ ! -x "$pet" ]]; then
    printf '未找到可执行程序，请先在项目目录执行 cargo build --release --locked --offline\n' >&2
    exit 1
fi
if [[ ! -S "$event_dir/events.sock" ]]; then
    printf '未找到事件接收端。请先在另一个终端执行并保持运行：\n' >&2
    printf '  cd %q\n  %q view %q\n' "$PWD" "$pet" "$event_dir" >&2
    printf '然后回到这里重试。demo 模式不接收事件；两个终端必须使用相同目录。\n' >&2
    exit 1
fi
printf 'FAKE task: three seconds of work, outcome=%s\n' "$outcome"
for tick in 1 2 3; do
    timeout 2s "$pet" emit "$event_dir" fake busy
    sleep 1
done
if [[ "$outcome" == fail ]]; then
    timeout 2s "$pet" emit "$event_dir" fake error
    printf 'FAKE task failed intentionally.\n' >&2
    exit 1
fi
timeout 2s "$pet" emit "$event_dir" fake done
printf 'FAKE task completed.\n'

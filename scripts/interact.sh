#!/usr/bin/env bash
# Explicit line-by-line manual controls; no capture of another terminal's input.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
event_dir=${1:-.pet-runtime}
if [[ $# -gt 1 ]]; then
    printf 'Usage: bash scripts/interact.sh [EVENT_DIR]\n' >&2
    exit 2
fi
pet=./target/release/avcii-sketch
if [[ ! -x "$pet" || ! -S "$event_dir/events.sock" ]]; then
    printf '请先构建程序，并在另一个终端保持运行：\n  %q view %q\n' "$pet" "$event_dir" >&2
    exit 1
fi
printf 'MANUAL 文字控制：提速 / 向左闪避 / 向右闪避 / 回中；输入 q 退出控制台。\n'
while IFS= read -r -p 'pet> ' text; do
    [[ "$text" == q || "$text" == exit || "$text" == 退出 ]] && break
    [[ -z "$text" ]] && continue
    if ! timeout 2s "$pet" say "$event_dir" "$text"; then
        printf '未发送成功：请检查上方提示。\n' >&2
    fi
done

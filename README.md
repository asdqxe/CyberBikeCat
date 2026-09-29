# ReJar Pet — 字符猫骑手（Linux 互动预览版）

状态：终端演示、文字事件及本地网页聊天预览已实现；尚未接入真实 Agent 或聊天宿主。
项目面向 NVIDIA 赛事，目标设备为 **NVIDIA DGX Spark**，以 Linux 为运行平台。
源码采用 [MIT 许可](LICENSE)，
AI 辅助与素材来源见 [AI_USE.md](AI_USE.md) 和 [THIRD_PARTY.md](THIRD_PARTY.md)。
当前提供源码预览；安装包和二进制 Release 尚未提供。实际验证范围见下文。

终端模式运行于 Linux 本地终端。角色保持跟随视角，城市道路沿纵深移动。
HUD 的 `SIMULATED input` 表示模拟活动：每 20 秒循环高活动 6 秒、
模拟 agent 忙碌 8 秒、低活动 3 秒、空闲 3 秒，驱动速度和角色姿态变化。
这是 `demo` 模式；新的 `view` 模式由明确标记的 MANUAL/FAKE 本地事件驱动。
没有真实打字活动采集、PTY 包装器或 Agent 接入；终端按键只用于退出。

当前外观为原创猫骑手：猫耳、后脑花纹、回头与伏低姿势，以及摆动的尾巴。
上空显示绿色 NVIDIA 字样和手绘简化眼形字符灯牌（不是官方图形资源）。
字符外框上方显示模拟输入与速度，下方显示 CAT 状态和退出按键。
小于 60 列或 24 行时隐藏灯牌，完整观察仍建议至少 100×30。

## 构建与观察

需要可用的 Rust/Cargo（源码语法至少需要 Rust 1.65，依赖还可能要求更高版本）
和 C 链接工具。缺少工具时，由用户手动安装；Debian 包安装命令为：

```sh
sudo apt install rustc cargo build-essential
```

在项目目录构建，首次构建需要下载依赖，保留生成的 `Cargo.lock`：

```sh
cargo build --release --locked
```

在支持 ANSI 真彩色的真实交互终端中运行，建议至少 100 列、30 行：

```sh
./target/release/avcii-sketch demo
```

观察约 20 秒的模拟活动循环，按 `q`、Esc 或 Ctrl-C 退出。
无参数启动仍保留相同行为。`--help` 查看用法，`--version` 查看版本；
这两个命令只输出文本。未知参数或多余参数会报错退出。
正常返回和 guard 建立后的错误返回会尝试恢复终端；SIGKILL 无法执行清理。
若异常终止后终端状态损坏，可在该终端手动执行 `stty sane` 和 `reset`。

## 网页聊天预览（Linux）

在已经构建的项目目录运行：

```sh
./target/release/avcii-sketch web
```

在本机浏览器打开终端打印的地址（默认 `http://127.0.0.1:8765`）。
不要为了预览把监听地址改为公网。端口占用时可用 `web 8766`，`web 0` 自动选空闲端口。
按启动终端的 Ctrl-C 停止服务；`web 0 --seconds 10` 可作有限运行。

页面接受任意文字提交，但只执行 **3 秒本地模拟任务**：提交提速、等待巡航、
完成 DONE、预设失败 ERROR、取消 CANCELLED。任务源标为 PREVIEW；文字只留在当前
页面，不发给服务器或模型，刷新页面不保留聊天内容。只允许一个进行中的预览任务。

`web/chat-feedback.js` 提供框架无关的 `createChatFeedback` 包装器，将整个异步请求
映射为提速、BUSY 心跳和 DONE/ERROR/CANCELLED。未来接真实后端时，请求必须在整个
回复流完成后才 resolve，并正确处理 AbortSignal；来源用 CHAT。**有包装器不代表
已经接入 Codex、Claude Code 或任何聊天宿主。**

服务仅监听 127.0.0.1，静态资源编译进程序，不提供任意文件读取，事件 POST 校验
Host/Origin。它是可信本机上的单会话开发预览，不是面向公网的 HTTP 服务，也不提供
身份认证或多用户隔离。网页模式使用本进程状态，不会自动连接另一个 `view` 进程。
改动 `web/` 资源后需要重新构建。`src/` 与 `web/` 必须一起提交，否则无法构建。

## 有限无头验证

```sh
./target/release/avcii-sketch --dump 8
timeout 2s ./target/release/avcii-sketch --dump NaN
timeout 2s ./target/release/avcii-sketch --dump inf
timeout 2s ./target/release/avcii-sketch --dump -1
timeout 2s ./target/release/avcii-sketch --dump 601
timeout 2s ./target/release/avcii-sketch --dump abc
timeout 2s ./target/release/avcii-sketch --dump
```

`--dump` 必须带有限的 0–600 秒参数；正常输出为 100×30 网格的纯文本单帧
（行末空格省略），无效参数应报错并立即非零退出，而非超时退出（124）。
静态帧不代表已观察彩色动画，也不验证交互退出或终端恢复。

## 本地事件接入（Linux）

打开两个终端，均在本项目目录操作。终端 A：

```sh
./target/release/avcii-sketch view .pet-runtime
```

终端 B 运行有限假任务：

```sh
bash scripts/fake-task.sh .pet-runtime ok
bash scripts/fake-task.sh .pet-runtime fail
```

两个命令各运行约 3 秒。`ok` 发送 BUSY → DONE，完成提示持续 2 秒后变 IDLE；
`fail` 发送 BUSY → ERROR，并故意退出 1。任意事件 5 秒未刷新后显示 DISCONNECTED，
这表示事件心跳过期，不是对真实 Agent 进程是否存活的判断。
也可手动发送 `./target/release/avcii-sketch emit .pet-runtime manual active`。

`view` 接收 idle/busy/done/error/cancelled/active，来源允许 manual/fake/preview/chat。
这些来源是显式事件标签；CHAT 本身不证明真实聊天适配已经完成。
另支持 boost/left/right/center 动作，或者通过 `say` 输入明确的中文字词。
通过 Unix datagram socket 通信，目录权限 0700、socket 权限 0600；没有网络监听。
发送成功表示消息已排队，不代表显示端已确认处理。一个目录只允许一个显示端。
目录应位于本项目内；正常退出会移除 socket、保留目录。若强制终止留下旧 socket，
确认旧进程已结束后使用新目录（如 `.pet-runtime-2`），不要删除仍在使用的端点。

终端 A 按 q、Esc 或 Ctrl-C 退出。尚未连接真实 Agent，不读取输入文本。
自动化检查可执行：

```sh
cargo test --locked --offline
timeout 20s bash scripts/check-events.sh
timeout 20s bash scripts/check-web.sh
```

两项脚本各自短时启动本地子进程，服务约 10 秒后退出；网页脚本另外需要 curl。
输出是 IPC/HTTP 状态记录，不是浏览器交互或动画观察。
若隔离环境拒绝 Unix socket，会明确失败，需要按运行环境审批或在真实终端手动执行。
本版已完成的检查及目标设备边界见下文「验证范围」。

## 文字互动

终端 A 保持 `./target/release/avcii-sketch view .pet-runtime` 运行。
若 A 是更新前启动的程序，先按 q 退出，再启动新版。
终端 B 可打开文字控制台：

```sh
bash scripts/interact.sh .pet-runtime
```

输入 `提速`、`向左闪避`、`向右闪避` 或 `回中`，回车发送；输入 q 只退出控制台。
也支持 `加速`、`冲刺`、`左`、`向左`、`左闪`、`右`、`向右`、`右闪`、`回正`，
以及 boost/left/right/center。它是明确词表，不会理解任意聊天句子；
例如“不要向左”会被拒绝，不会误触发。没有模型调用或其他窗口的文字监听。

也可以直接调用：

```sh
./target/release/avcii-sketch say .pet-runtime 提速
./target/release/avcii-sketch say .pet-runtime 向左闪避
./target/release/avcii-sketch say .pet-runtime 向右闪避
./target/release/avcii-sketch say .pet-runtime 回中
```

提速持续 2 秒；左右闪避目标持续 0.9 秒后平滑回中。再次发送会刷新对应动作时限，
提速和闪避可以叠加，不覆盖 BUSY/DONE 等任务状态，也不延长任务心跳。
下栏分别显示转向、提速的控制来源；没有有效任务心跳但有动作时显示 CONTROL，
动作结束后恢复 DISCONNECTED。车辆移动仍为视觉反馈，没有碰撞或障碍物判定。

首次从 GitHub 获取源码的用户，应先运行 `cargo build --release --locked` 下载锁定依赖；
依赖已有缓存后才使用 `--offline`。目前是源码预览，不承诺免构建安装。

## 验证范围

截至 2026-09-29，已完成以下源码预览检查：

- **Linux x86_64**：锁定依赖离线 release 构建、8 项 Rust 单元测试、IPC 与 HTTP 检查通过。
- **终端**：自动化 PTY 检查覆盖 20 秒运行、q / Esc / Ctrl-C 退出、终端状态恢复及 socket 清理。
- **本地网页**：12 项 Chrome 自动化检查通过，覆盖提交、完成、失败、取消、重试、
  文本与网络边界、动画帧变化及桌面/窄屏布局；已复核实际截图。

**NVIDIA DGX Spark 是参赛目标设备，其 Linux ARM64 实机构建与运行尚未核实
（NOT_VERIFIED）。上述 x86_64 结果不代表目标设备验收通过。**
当前未集成真实模型、CUDA 推理或真实 Agent；不宣称已发挥目标设备的 GPU 加速能力。

`sketches/wgpu_backend_sketch.rs` 仅为未编译草图，不属于已验证运行路径。
本仓库发布源码与使用说明；本地开发日志、运行证据和构建产物不纳入公开提交。

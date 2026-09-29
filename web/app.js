import { createChatFeedback } from "/chat-feedback.js";

const $ = (id) => document.getElementById(id);
const canvas = $("pet");
const ctx = canvas.getContext("2d");
ctx.font = "15px monospace";
ctx.textBaseline = "top";

async function localFetch(path, options = {}) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1500);
  try {
    const response = await fetch(path, { ...options, signal: controller.signal, cache: "no-store" });
    if (!response.ok) throw new Error(`本地连接失败 (${response.status})`);
    return await response.text();
  } finally { clearTimeout(timer); }
}

async function draw() {
  try {
    if (!document.hidden) {
      const frame = JSON.parse(await localFetch("/frame"));
      ctx.fillStyle = "#080e12";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      for (let i = 0; i < frame.text.length; i++) {
        if (frame.text[i] === " ") continue;
        ctx.fillStyle = `#${frame.colors[i].toString(16).padStart(6, "0")}`;
        ctx.fillText(frame.text[i], (i % frame.cols) * 10, Math.floor(i / frame.cols) * 18);
      }
      $("state").textContent = `${frame.source} / ${frame.state}`;
      $("connection").textContent = "本地渲染器已连接";
    }
  } catch {
    $("connection").textContent = "连接已断开，请检查启动程序的终端";
  } finally { setTimeout(draw, document.hidden ? 500 : 50); }
}
void draw();

const withPet = createChatFeedback({
  source: "preview",
  emit: (source, state) => localFetch("/event", {
    method: "POST", headers: { "Content-Type": "text/plain;charset=UTF-8" }, body: `${source} ${state}`,
  }),
  onFeedbackError: () => { $("feedback-error").textContent = "宠物反馈连接失败；请检查本地服务。"; },
});

function message(text, type) {
  const element = document.createElement("div");
  element.className = `message ${type}`;
  element.textContent = text; // user text must never become HTML
  $("messages").append(element);
  element.scrollIntoView({ block: "nearest" });
}

// Replace this callback with YOUR backend request. It must resolve only when the
// entire reply stream finishes, reject on failure, and honour AbortSignal.
function previewRequest(outcome, signal) {
  return new Promise((resolve, reject) => {
    const cancel = () => { clearTimeout(timer); reject(new DOMException("Cancelled", "AbortError")); };
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", cancel);
      if (outcome === "fail") reject(new Error("本地预览的预设失败"));
      else resolve("预览任务完成。这不是模型回答；输入内容没有离开本页。");
    }, 3000);
    signal.addEventListener("abort", cancel, { once: true });
    if (signal.aborted) cancel();
  });
}

let active;
$("cancel").addEventListener("click", () => active?.abort());
$("chat-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const prompt = $("prompt").value.trim();
  if (!prompt || active) return;
  const outcome = $("outcome").value;
  message(prompt, "user");
  $("prompt").value = "";
  $("feedback-error").textContent = "";
  active = new AbortController();
  $("send").disabled = true;
  $("cancel").disabled = false;
  try {
    const reply = await withPet((signal) => previewRequest(outcome, signal), { signal: active.signal });
    message(reply, "reply");
  } catch (error) {
    message(error.name === "AbortError" ? "任务已取消。" : `预览失败：${error.message}`, "error");
  } finally {
    active = undefined;
    $("send").disabled = false;
    $("cancel").disabled = true;
  }
});

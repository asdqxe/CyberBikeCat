// Framework-neutral: wrap the promise that represents the WHOLE chat request.
// emit receives only source/state, never prompt contents. One request at a time.
export function createChatFeedback({ emit, source = "chat", onFeedbackError = () => {} }) {
  let running = false;
  let queue = Promise.resolve();
  const send = (state) => {
    queue = queue.then(() => emit(source, state)).catch((error) => {
      try { onFeedbackError(error); } catch { /* feedback must not break chat */ }
    });
    return queue;
  };
  return async function withPet(request, { signal } = {}) {
    if (running) throw new Error("A chat request is already running");
    if (signal?.aborted) throw new DOMException("Cancelled", "AbortError");
    running = true;
    let heartbeat;
    let onAbort;
    try {
      // Queue order preserves submitted -> busy -> done/error even on fast replies.
      void send("boost");
      void send("busy");
      let heartbeatPending = false;
      heartbeat = setInterval(() => {
        if (heartbeatPending) return;
        heartbeatPending = true;
        void send("busy").finally(() => { heartbeatPending = false; });
      }, 1000);
      const aborted = new Promise((_, reject) => {
        onAbort = () => reject(new DOMException("Cancelled", "AbortError"));
        signal?.addEventListener("abort", onAbort, { once: true });
      });
      const result = await Promise.race([Promise.resolve().then(() => request(signal)), aborted]);
      clearInterval(heartbeat);
      void send("done");
      return result;
    } catch (error) {
      clearInterval(heartbeat);
      void send(signal?.aborted || error?.name === "AbortError" ? "cancelled" : "error");
      throw error;
    } finally {
      clearInterval(heartbeat);
      signal?.removeEventListener("abort", onAbort);
      running = false;
    }
  };
}

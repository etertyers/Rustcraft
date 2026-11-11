// A Rustcraft worker (M72): the same wasm as the page, in its own instance.
// The page posts {type: "init", kind} first, then {id, bytes} requests.
import init, { worker_generate } from "./pkg/rustcraft.js";

let ready = null;
self.onmessage = async (event) => {
  const msg = event.data;
  if (msg.type === "init") {
    ready = init().then(() => self.postMessage({ type: "ready", kind: msg.kind }));
    return;
  }
  await ready;
  if (msg.bytes) {
    const out = worker_generate(new Uint8Array(msg.bytes));
    if (out) {
      self.postMessage({ id: msg.id, bytes: out }, [out.buffer]);
    }
  }
};

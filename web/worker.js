// A Rustcraft worker (M72): the same wasm as the page, in its own instance.
// The page posts {type: "init", kind} first, then {id, bytes} requests.
import init, { worker_generate, server_worker_run, server_worker_save, server_worker_close } from "./pkg/rustcraft.js";

let ready = null;
self.onmessage = async (event) => {
  const msg = event.data;
  if (msg.type === "init") {
    ready = init().then(() => self.postMessage({ type: "ready", kind: msg.kind }));
    return;
  }
  await ready;
  if (msg.type === "server") {
    // The server for one world (M76): the page's end of a MessageChannel and
    // the world to play.
    server_worker_run(msg.port, msg.assetsUrl, new Uint8Array(msg.spec)).catch((e) =>
      console.error("server worker:", e),
    );
    return;
  }
  if (msg.type === "save") return server_worker_save();
  if (msg.type === "close") {
    await server_worker_close();
    self.postMessage({ type: "closed" });
    return;
  }
  if (msg.bytes) {
    const out = worker_generate(new Uint8Array(msg.bytes));
    if (out) {
      self.postMessage({ id: msg.id, bytes: out }, [out.buffer]);
    }
  }
};

// Dev helper: evaluate JS inside a running oto WebView2 window over the Chrome DevTools Protocol.
// Launch the app with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333.
// Usage: node scripts/cdp.mjs <window-label-or-url-part> "<js expression>"
const [, , match = "", expr = "document.title"] = process.argv;
const targets = await (await fetch("http://127.0.0.1:9333/json")).json();
const pages = targets.filter((t) => t.type === "page");
const pick = [];
for (const t of pages) {
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((r) => (ws.onopen = r));
  const label = await call(ws, "document.documentElement.dataset.window");
  ws.close();
  if (!match || label === match) pick.push(t);
}
if (!pick.length) { console.log("no window", match, pages.map((p) => p.url)); process.exit(1); }
const ws = new WebSocket(pick[0].webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
console.log(JSON.stringify(await call(ws, expr), null, 1));
ws.close();

function call(ws, expression) {
  return new Promise((resolve) => {
    const id = Math.floor(Math.random() * 1e9);
    const on = (m) => {
      const d = JSON.parse(m.data);
      if (d.id !== id) return;
      ws.removeEventListener("message", on);
      resolve(d.result?.result?.value ?? d.result?.exceptionDetails?.exception?.description ?? d);
    };
    ws.addEventListener("message", on);
    ws.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
  });
}

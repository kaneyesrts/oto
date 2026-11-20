// Dev helper: screenshot an oto window over CDP. node scripts/cdp-shot.mjs <label> <out.png>
import { writeFileSync } from "node:fs";
const [, , label = "main", out = "shot.png"] = process.argv;
const pages = (await (await fetch("http://127.0.0.1:9333/json")).json()).filter((t) => t.type === "page");
for (const t of pages) {
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((r) => (ws.onopen = r));
  let id = 0;
  const send = (method, params) => new Promise((res) => {
    const my = ++id;
    const on = (m) => { const d = JSON.parse(m.data); if (d.id === my) { ws.removeEventListener("message", on); res(d.result); } };
    ws.addEventListener("message", on);
    ws.send(JSON.stringify({ id: my, method, params }));
  });
  const v = (await send("Runtime.evaluate", { expression: "document.documentElement.dataset.window", returnByValue: true }))?.result?.value;
  if (v !== label) { ws.close(); continue; }
  const r = await send("Page.captureScreenshot", { format: "png" });
  writeFileSync(out, Buffer.from(r.data, "base64"));
  console.log("saved", out);
  ws.close();
}

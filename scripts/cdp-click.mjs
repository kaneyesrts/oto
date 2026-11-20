// Dev helper: real mouse click/drag on an element inside the main oto window via CDP.
// Usage: node scripts/cdp-click.mjs "<css selector>" <fraction-x 0..1>
const [, , sel, fx = "0.5"] = process.argv;
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
  const ev = async (e) => (await send("Runtime.evaluate", { expression: e, returnByValue: true, awaitPromise: true }))?.result?.value;
  if ((await ev("document.documentElement.dataset.window")) !== "main") { ws.close(); continue; }
  const r = await ev(`(()=>{const e=document.querySelector(${JSON.stringify(sel)}); if(!e) return null; const b=e.getBoundingClientRect(); return {x:b.x,y:b.y,w:b.width,h:b.height}})()`);
  if (!r) { console.log("selector not found"); process.exit(1); }
  const x = r.x + r.w * +fx, y = r.y + r.h / 2;
  const before = await ev("window.__TAURI_INTERNALS__.invoke('player_state').then(s=>s.posMs)");
  await send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 });
  await new Promise((r) => setTimeout(r, 700));
  const after = await ev("window.__TAURI_INTERNALS__.invoke('player_state').then(s=>({pos:s.posMs,dur:s.durationMs}))");
  console.log(JSON.stringify({ rect: r, before, after }));
  ws.close();
}

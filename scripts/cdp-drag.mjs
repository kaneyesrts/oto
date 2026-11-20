// Dev helper: drag across an element in the main window: node scripts/cdp-drag.mjs "<sel>" 0.2 0.8 [pause]
const [, , sel, a = "0.2", b = "0.8", pause] = process.argv;
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
  if ((await ev("document.documentElement.dataset.window")) !== (process.env.WIN || "main")) { ws.close(); continue; }
  if (pause) await ev("window.__TAURI_INTERNALS__.invoke('player',{req:{type:'pause'}})");
  const r = await ev(`(()=>{const b=document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return {x:b.x,y:b.y,w:b.width,h:b.height}})()`);
  const y = r.y + r.h / 2, xa = r.x + r.w * +a, xb = r.x + r.w * +b;
  await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: xa, y });
  await send("Input.dispatchMouseEvent", { type: "mousePressed", x: xa, y, button: "left", buttons: 1, clickCount: 1 });
  for (let i = 1; i <= 10; i++) await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: xa + ((xb - xa) * i) / 10, y, button: "left", buttons: 1 });
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", x: xb, y, button: "left", buttons: 0, clickCount: 1 });
  await new Promise((r) => setTimeout(r, 900));
  console.log(JSON.stringify(await ev("window.__TAURI_INTERNALS__.invoke('player_state').then(s=>({pos:s.posMs,dur:s.durationMs,playing:s.playing}))")));
  ws.close();
}

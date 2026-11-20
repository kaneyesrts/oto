import "./app.css";
import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { init } from "./lib/store.svelte";

async function boot() {
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    (await import("./dev/mock")).installMock();
  }
  const label = getCurrentWebviewWindow().label;
  document.documentElement.dataset.window = label;
  const target = document.getElementById("app")!;
  if (label === "widget") {
    await init(false);
    const { default: Widget } = await import("./windows/Widget.svelte");
    mount(Widget, { target });
  } else if (label === "flyout") {
    await init(false);
    const { default: Flyout } = await import("./windows/Flyout.svelte");
    mount(Flyout, { target });
  } else {
    await init(true);
    const { default: App } = await import("./App.svelte");
    mount(App, { target });
  }
}

// Block the browser's own context menu outside text fields; it breaks the app illusion.
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (t.tagName !== "INPUT" && t.tagName !== "TEXTAREA") e.preventDefault();
});

boot();

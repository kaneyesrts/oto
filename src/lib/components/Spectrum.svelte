<script lang="ts">
  import { onMount } from "svelte";
  import { frame } from "../store.svelte";

  let {
    bars = 32,
    mode = "bars" as "bars" | "mirror" | "wave",
    color = "var(--accent)",
    gap = 2,
    radius = 2,
    class: klass = "",
  } = $props();

  let canvas: HTMLCanvasElement;

  onMount(() => {
    const ctx = canvas.getContext("2d")!;
    let raf = 0;
    const smooth = new Float32Array(bars);
    const loop = () => {
      raf = requestAnimationFrame(loop);
      const dpr = Math.min(devicePixelRatio || 1, 2);
      const w = canvas.clientWidth, h = canvas.clientHeight;
      if (!w || !h) return;
      if (canvas.width !== Math.round(w * dpr)) { canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr); }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      ctx.fillStyle = getComputedStyle(canvas).color;
      ctx.strokeStyle = ctx.fillStyle;
      // Resample 48 analyser bands to the requested bar count.
      for (let i = 0; i < bars; i++) {
        const a = (i / bars) * frame.b.length, b = ((i + 1) / bars) * frame.b.length;
        let m = 0;
        for (let k = Math.floor(a); k < Math.max(Math.floor(a) + 1, Math.floor(b)); k++) m = Math.max(m, frame.b[k] ?? 0);
        smooth[i] += (m / 255 - smooth[i]) * 0.5;
      }
      if (mode === "wave") {
        ctx.lineWidth = 2;
        ctx.lineCap = "round";
        ctx.beginPath();
        for (let i = 0; i < bars; i++) {
          const x = (i / (bars - 1)) * w;
          const y = h / 2 + Math.sin(i * 1.7 + performance.now() / 260) * smooth[i] * h * 0.42;
          i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
        }
        ctx.stroke();
        return;
      }
      const bw = (w - gap * (bars - 1)) / bars;
      for (let i = 0; i < bars; i++) {
        const v = Math.max(0.04, smooth[i]);
        const bh = v * (mode === "mirror" ? h : h);
        const x = i * (bw + gap);
        const y = mode === "mirror" ? (h - bh) / 2 : h - bh;
        ctx.globalAlpha = 0.35 + v * 0.65;
        ctx.beginPath();
        ctx.roundRect(x, y, bw, bh, Math.min(radius, bw / 2));
        ctx.fill();
      }
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  });
</script>

<canvas bind:this={canvas} class={klass} style="color:{color}"></canvas>

<style>
  canvas { display: block; width: 100%; height: 100%; }
</style>

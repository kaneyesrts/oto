// Liquid glass: real refraction at the edges of a rounded shape, done with an SVG
// displacement map applied through `backdrop-filter: url(#…)` (Chromium/WebView2 only).
// Technique after shuding/liquid-glass and archisvaze/liquid-glass: build a per-element
// displacement map from the signed distance to the rounded-rect edge with a convex
// "squircle lens" profile, so content bends inward near the rim like a thick glass slab.

let host: SVGSVGElement | null = null;
let seq = 0;

function svgHost() {
  if (host) return host;
  host = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  host.setAttribute("width", "0");
  host.setAttribute("height", "0");
  host.style.position = "absolute";
  host.style.pointerEvents = "none";
  document.body.appendChild(host);
  return host;
}

function sdRoundRect(px: number, py: number, w: number, h: number, r: number) {
  const qx = Math.abs(px - w / 2) - (w / 2 - r);
  const qy = Math.abs(py - h / 2) - (h / 2 - r);
  const ox = Math.max(qx, 0), oy = Math.max(qy, 0);
  return Math.min(Math.max(qx, qy), 0) + Math.hypot(ox, oy) - r;
}

/** Builds the displacement map at reduced resolution; the filter scales it up smoothly. */
function displacementMap(w: number, h: number, radius: number, bezel: number) {
  const k = Math.max(1, Math.round(Math.max(w, h) / 220)); // downsample factor
  const cw = Math.max(2, Math.round(w / k)), ch = Math.max(2, Math.round(h / k));
  const c = document.createElement("canvas");
  c.width = cw; c.height = ch;
  const ctx = c.getContext("2d")!;
  const img = ctx.createImageData(cw, ch);
  const r = Math.min(radius, w / 2, h / 2);
  for (let y = 0; y < ch; y++) {
    for (let x = 0; x < cw; x++) {
      const px = (x + 0.5) * k, py = (y + 0.5) * k;
      const d = -sdRoundRect(px, py, w, h, r); // distance inside, px
      let dx = 0, dy = 0;
      if (d > 0 && d < bezel) {
        // Outward normal via central differences.
        const e = 1;
        const nx = sdRoundRect(px + e, py, w, h, r) - sdRoundRect(px - e, py, w, h, r);
        const ny = sdRoundRect(px, py + e, w, h, r) - sdRoundRect(px, py - e, w, h, r);
        const nl = Math.hypot(nx, ny) || 1;
        const t = 1 - d / bezel; // 0 inner → 1 rim
        // Convex squircle profile: flat in the middle, steep at the rim.
        const mag = 1 - Math.pow(1 - Math.pow(t, 4), 0.25);
        dx = -(nx / nl) * mag;
        dy = -(ny / nl) * mag;
      }
      const i = (y * cw + x) * 4;
      img.data[i] = 128 + dx * 127;
      img.data[i + 1] = 128 + dy * 127;
      img.data[i + 2] = 128;
      img.data[i + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
  return c.toDataURL();
}

export interface GlassOpts {
  radius?: number;
  bezel?: number;
  /** Max displacement in px. */
  strength?: number;
  frost?: number;
  /** Chromatic aberration: extra displacement for the red channel. */
  dispersion?: number;
  enabled?: boolean;
}

/** Svelte action: `use:glass={{ radius: 28 }}` */
export function glass(node: HTMLElement, opts: GlassOpts = {}) {
  const id = `lg-${++seq}`;
  const svg = svgHost();
  const filter = document.createElementNS("http://www.w3.org/2000/svg", "filter");
  filter.setAttribute("id", id);
  filter.setAttribute("color-interpolation-filters", "sRGB");
  filter.setAttribute("x", "0");
  filter.setAttribute("y", "0");
  filter.setAttribute("width", "100%");
  filter.setAttribute("height", "100%");
  svg.appendChild(filter);

  let o = { radius: 28, bezel: 22, strength: 38, frost: 1.2, dispersion: 6, enabled: true, ...opts };
  let lastKey = "";

  const build = () => {
    const w = Math.round(node.offsetWidth), h = Math.round(node.offsetHeight);
    node.classList.toggle("liquid-glass", !!o.enabled);
    if (!o.enabled || w < 4 || h < 4) {
      node.style.backdropFilter = "";
      lastKey = "";
      return;
    }
    const key = `${w}x${h}:${o.radius}:${o.bezel}:${o.strength}:${o.frost}:${o.dispersion}`;
    if (key === lastKey) return;
    lastKey = key;
    const map = displacementMap(w, h, o.radius, Math.min(o.bezel, Math.min(w, h) / 2 - 1));
    const s = o.strength, d = o.dispersion;
    filter.innerHTML = `
      <feImage href="${map}" x="0" y="0" width="${w}" height="${h}" preserveAspectRatio="none" result="map"/>
      <feGaussianBlur in="SourceGraphic" stdDeviation="${o.frost}" result="frost"/>
      <feDisplacementMap in="frost" in2="map" scale="${s + d}" xChannelSelector="R" yChannelSelector="G" result="dr"/>
      <feColorMatrix in="dr" type="matrix" values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0" result="r"/>
      <feDisplacementMap in="frost" in2="map" scale="${s}" xChannelSelector="R" yChannelSelector="G" result="dg"/>
      <feColorMatrix in="dg" type="matrix" values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0" result="g"/>
      <feDisplacementMap in="frost" in2="map" scale="${s - d}" xChannelSelector="R" yChannelSelector="G" result="db"/>
      <feColorMatrix in="db" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0" result="b"/>
      <feBlend in="r" in2="g" mode="screen" result="rg"/>
      <feBlend in="rg" in2="b" mode="screen"/>`;
    node.style.backdropFilter = `url(#${id}) saturate(1.5) brightness(1.06)`;
  };

  node.style.setProperty("--lg-radius", `${o.radius}px`);
  const ro = new ResizeObserver(() => build());
  ro.observe(node);
  build();

  return {
    update(next: GlassOpts = {}) {
      o = { ...o, ...next };
      node.style.setProperty("--lg-radius", `${o.radius}px`);
      lastKey = "";
      build();
    },
    destroy() {
      ro.disconnect();
      filter.remove();
      node.style.backdropFilter = "";
      node.classList.remove("liquid-glass");
    },
  };
}

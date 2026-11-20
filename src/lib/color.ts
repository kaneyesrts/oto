import type { RGB } from "./api";

export function rgbToHsl([r, g, b]: RGB): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = 0;
  if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  return [h * 60, s, l];
}

export function hslToRgb(h: number, s: number, l: number): RGB {
  h = ((h % 360) + 360) % 360 / 360;
  if (s === 0) return [l * 255, l * 255, l * 255].map(Math.round) as RGB;
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const f = (t: number) => {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return [f(h + 1 / 3), f(h), f(h - 1 / 3)].map((x) => Math.round(x * 255)) as RGB;
}

const css = (c: RGB) => `${c[0]} ${c[1]} ${c[2]}`;

/** Picks the most vivid colour from a palette and nudges its lightness so it reads on the
 *  theme's background. Grey covers fall back to the theme's own accent. */
export function accentFrom(palette: RGB[], dark: boolean): RGB | null {
  if (!palette?.length) return null;
  let best: RGB | null = null;
  let score = 0;
  palette.forEach((c, i) => {
    const [, s, l] = rgbToHsl(c);
    const sc = s * (1 - Math.abs(l - 0.55)) * (1 - i * 0.06);
    if (sc > score) { score = sc; best = c; }
  });
  if (!best || score < 0.12) return null;
  const [h, s] = rgbToHsl(best);
  return hslToRgb(h, Math.min(0.85, Math.max(0.5, s)), dark ? 0.62 : 0.46);
}

export function applyPalette(palette: RGB[] | undefined, dark: boolean, enabled: boolean) {
  const root = document.documentElement;
  const acc = enabled ? accentFrom(palette ?? [], dark) : null;
  if (acc) root.style.setProperty("--accent-rgb", css(acc));
  else root.style.removeProperty("--accent-rgb");
  // Ambient colours for gradients/backgrounds always come from the cover when available.
  const pal = palette?.length ? palette : null;
  for (let i = 0; i < 3; i++) {
    const c = pal ? pal[i % pal.length] : null;
    if (c) root.style.setProperty(`--amb-${i}`, css(c));
    else root.style.removeProperty(`--amb-${i}`);
  }
}

/** Palette as normalised floats for shaders, padded to 4 colours. */
export function shaderPalette(palette: RGB[] | undefined, fallback: RGB[]): number[] {
  const p = palette?.length ? palette : fallback;
  const out: number[] = [];
  for (let i = 0; i < 4; i++) {
    const c = p[i % p.length];
    out.push(c[0] / 255, c[1] / 255, c[2] / 255);
  }
  return out;
}

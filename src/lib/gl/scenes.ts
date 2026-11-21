// Immersive "spaces" behind the lyrics. Each scene is one fragment shader fed by the same
// inputs: the cover palette (made vivid, so it can glow), the audio analysis (level, bass,
// mids, beats, 16 spectrum bands) and a "travel" distance that advances faster when the
// music is loud. "aura" is the original cover-gradient mesh and stays the default.

import { frame } from "../store.svelte";
import { program, mesh } from "./mesh";
import { rgbToHsl, hslToRgb } from "../color";
import type { RGB, SceneId } from "../api";

const VERT = `#version 300 es
in vec2 p; out vec2 uv;
void main(){ uv = p * 0.5 + 0.5; gl_Position = vec4(p, 0.0, 1.0); }`;

const HEAD = `#version 300 es
precision highp float;
in vec2 uv; out vec4 o;
uniform float uT, uZ, uLvl, uBass, uMid, uHigh, uBeat, uAmt;
uniform vec2 uRes;
uniform vec3 uC[4];
uniform float uS[16];
const float TAU = 6.2831853;

float hash(vec2 p){ p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
float noise(vec2 p){
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1,0)), u.x), mix(hash(i + vec2(0,1)), hash(i + vec2(1,1)), u.x), u.y);
}
float fbm(vec2 p){ float v = 0.0, a = 0.5; for(int i = 0; i < 4; i++){ v += a * noise(p); p = p * 2.03 + 17.1; a *= 0.5; } return v; }
vec3 pal(float x){
  x = fract(x) * 4.0;
  int i = int(floor(x));
  return mix(uC[i], uC[(i + 1) % 4], smoothstep(0.0, 1.0, fract(x)));
}
float band(float x){ return uS[int(clamp(x, 0.0, 0.999) * 16.0)]; }
// 1 at d = 0, 0 at d >= r (smoothstep with reversed edges is undefined in GLSL).
float disc(float d, float r){ return 1.0 - smoothstep(0.0, r, d); }
`;

const MAIN = `
void main(){
  vec2 p = (uv - 0.5) * vec2(uRes.x / uRes.y, 1.0) * 2.0;
  vec3 col = scene(uv, p);
  col = 1.0 - exp(-col * 1.25);                       // filmic shoulder: glows, never clips
  vec2 v = uv - 0.5;
  col *= 1.0 - dot(v, v) * 0.7;                       // vignette
  col += (hash(uv * uRes + fract(uT)) - 0.5) * 0.014; // grain against banding
  o = vec4(max(col, 0.0), 1.0);
}`;

// Crisp point stars at a fixed on-screen size, for any grid scale.
const STARS = `
vec3 stars(vec2 p, float scale, float seed, float density, float bright){
  vec2 g = p * scale + seed * 23.7;
  vec2 id = floor(g);
  float h = hash(id + seed * 7.1);
  vec2 off = (vec2(hash(id + 3.3), hash(id + 5.9)) - 0.5) * 0.6;
  float d = length(fract(g) - 0.5 - off) / scale;
  float px = 2.0 / uRes.y;
  float rad = px * (0.55 + 1.3 * h * h);
  float core = exp(-pow(d / rad, 2.0));
  float halo = exp(-d / (rad * 3.0)) * 0.07;
  float tw = 0.65 + 0.35 * sin(uT * (1.5 + h * 3.0) + h * 40.0);
  vec3 tint = mix(vec3(1.0), pal(h * 3.0), 0.3);
  return tint * (core + halo) * tw * step(1.0 - density, h) * bright;
}
`;

const SCENES: Record<Exclude<SceneId, "aura">, string> = {
  // Deep space: a dim nebula with dust lanes in the cover's colours, stars drifting past.
  space: STARS + `
vec3 scene(vec2 uv, vec2 p){
  vec2 q = p * 0.7 + vec2(uT * 0.004, uT * 0.002);
  vec2 w = vec2(fbm(q * 1.6 + uT * 0.006), fbm(q * 1.6 + 4.3 - uT * 0.005));
  float n = fbm(q * 1.2 + w * 1.4);
  float dust = fbm(q * 3.0 + w * 2.0 + 8.1);
  vec3 neb = mix(uC[0], uC[2], smoothstep(0.3, 0.8, w.x));
  float dens = smoothstep(0.3, 0.95, n) * (0.5 + 0.5 * dust);
  vec3 col = vec3(0.005, 0.006, 0.014) + neb * dens * 0.6 * (0.8 + uBass * 0.5 * uAmt);
  col *= 1.0 - smoothstep(0.55, 0.85, dust) * 0.55;
  for (int i = 0; i < 4; i++) {
    float fi = float(i);
    float depth = fract(fi * 0.25 + uZ);
    float fade = smoothstep(0.0, 0.3, depth) * (1.0 - smoothstep(0.82, 1.0, depth));
    col += stars(p, mix(22.0, 4.0, depth), fi, 0.22, fade * (0.9 + uBeat * 0.5 * uAmt));
  }
  return col;
}`,
  // Silk: soft ribbons of light folding slowly; the bass makes them ripple.
  silk: `
vec3 scene(vec2 uv, vec2 p){
  vec3 col = vec3(0.006, 0.006, 0.01) + uC[3] * 0.015;
  for (int i = 0; i < 4; i++) {
    float fi = float(i);
    float t = uT * (0.05 + fi * 0.012);
    float x = p.x;
    float y = 0.32 * sin(x * 0.9 + t * 2.0 + fi * 1.7) + 0.16 * sin(x * 1.7 - t * 1.3 + fi)
            + 0.07 * sin(x * 3.1 + t * 0.7) * (1.0 + uBass * 1.5 * uAmt);
    y += (fi - 1.5) * 0.2;
    float width = 0.2 + 0.08 * sin(x * 0.7 + t + fi);
    float d = (p.y - y) / width;
    float body = exp(-d * d * 1.6);
    float sheen = pow(0.5 + 0.5 * sin(d * 2.6 + x * 1.8 + t * 3.0), 4.0);
    col += pal(fi * 0.27 + x * 0.05) * body * (0.05 + 0.36 * sheen) * (0.85 + uMid * 0.5 * uAmt);
  }
  return col;
}`,
  // Northern lights: thin vertical rays rising from a bright lower edge over a night sky.
  aurora: STARS + `
vec3 scene(vec2 uv, vec2 p){
  vec3 col = mix(vec3(0.016, 0.022, 0.05), vec3(0.003, 0.004, 0.01), uv.y);
  col += stars(p, 16.0, 9.0, 0.2, 0.5 * smoothstep(-0.2, 0.8, p.y));
  for (int k = 0; k < 3; k++) {
    float fk = float(k);
    float x = p.x * 0.55 + fk * 2.3;
    // Curtains fold: a slow large wave plus finer ripples, so the lower edge is never straight.
    float wob = fbm(vec2(x * 1.4 + uT * 0.025, fk * 1.7 + uT * 0.012));
    float edge = -0.05 + fk * 0.2 + 0.28 * sin(x * 1.3 + uT * 0.04 + fk * 2.0) + (wob - 0.5) * 0.45;
    float dy = p.y - edge;
    float vert = smoothstep(-0.03, 0.04, dy) * exp(-max(dy, 0.0) * (2.2 + fk * 0.6));
    // Soft rays that drift sideways along the curtain.
    float rays = 0.45 + 0.55 * smoothstep(0.25, 0.85, fbm(vec2(x * 7.0 + wob * 3.0 - uT * 0.03, fk * 4.0)));
    float sp = band(fract(uv.x * 0.8 + fk * 0.3));
    vec3 c = pal(fk * 0.33 + 0.1);
    col += c * vert * rays * (0.2 + 0.26 * sp * uAmt + 0.1 * uMid * uAmt);
  }
  return col;
}`,
  // Lights out of focus, like a stage through a wide-open lens: sparse, soft, with bright rims.
  depth: `
vec3 scene(vec2 uv, vec2 p){
  vec3 col = vec3(0.008, 0.007, 0.012) + uC[1] * 0.025 * (0.6 + fbm(p * 0.8 + uT * 0.01));
  for (int i = 0; i < 4; i++) {
    float fi = float(i);
    float dp = fi / 3.0;
    float scale = mix(5.0, 1.6, dp);
    vec2 g = p * scale + vec2(fi * 13.1 + sin(uT * 0.05 + fi) * 0.4, -uZ * mix(1.0, 3.0, dp) + fi * 7.3);
    vec2 id = floor(g);
    float h = hash(id + fi * 3.3);
    float d = length(fract(g) - 0.5 - (vec2(hash(id + 2.1), hash(id + 5.3)) - 0.5) * 0.4);
    float rad = 0.12 + 0.14 * h;
    float soft = mix(0.012, 0.12, clamp(abs(dp - 0.35) * 1.5, 0.0, 1.0));
    float disk = 1.0 - smoothstep(rad - soft, rad + soft * 0.2, d);
    float rim = exp(-pow((d - rad + soft * 0.5) / soft, 2.0) * 2.0) * 0.35;
    float tw = 0.75 + 0.25 * sin(uT * 0.6 + h * 30.0);
    col += pal(h * 2.0 + fi * 0.15) * (disk * 0.55 + rim) * step(0.78, h) * tw * mix(0.12, 0.3, dp)
         * (0.8 + uBass * 0.7 * uAmt + uBeat * 0.4 * uAmt);
  }
  return col;
}`,
  // Smoke drifting upwards, lit from below by a light that pulses with the bass.
  smoke: `
vec3 scene(vec2 uv, vec2 p){
  vec2 q = p * 0.9;
  q.y -= uT * 0.03;
  vec2 w = vec2(fbm(q * 1.5 + vec2(0.0, uT * 0.02)), fbm(q * 1.5 + 5.2 - vec2(uT * 0.015, 0.0)));
  float n = fbm(q * 1.1 + w * 1.8);
  float n2 = fbm(q * 2.3 + w * 2.5 + 3.0);
  float dens = smoothstep(0.3, 0.9, n) * (0.6 + 0.4 * n2);
  float light = exp(-pow(p.y + 1.1, 2.0) * 0.8) * (0.7 + uBass * 0.8 * uAmt) + 0.2;
  vec3 c = mix(uC[0], uC[1], w.x);
  vec3 col = vec3(0.005) + c * dens * light * 0.6;
  col += uC[0] * exp(-length(p - vec2(0.0, -1.2)) * 1.5) * 0.05 * (1.0 + uBeat * uAmt);
  return col;
}`,
  // Almost nothing: darkness with a slow breath of the cover's colour. For the words alone.
  void: `
vec3 scene(vec2 uv, vec2 p){
  float r = length(p * vec2(0.8, 1.0));
  vec3 col = uC[0] * 0.09 * exp(-r * 1.3) * (0.6 + uBass * 1.0 * uAmt);
  col += uC[2] * 0.035 * exp(-length(p - vec2(1.1, -0.7)) * 1.4);
  return col;
}`,
};

/** Cover colours pushed to glowing, saturated versions (grey covers get a soft violet). */
export function vividPalette(palette: RGB[] | undefined): number[] {
  const src = palette?.length ? palette : ([[150, 110, 255], [255, 120, 170], [90, 160, 255], [255, 170, 120]] as RGB[]);
  const out: number[] = [];
  for (let i = 0; i < 4; i++) {
    const [h, s] = rgbToHsl(src[i % src.length]);
    const c = hslToRgb(h, Math.max(0.35, Math.min(0.75, s * 1.2)), 0.56);
    out.push(c[0] / 255, c[1] / 255, c[2] / 255);
  }
  return out;
}

export interface SceneOpts {
  id: SceneId;
  palette: () => RGB[] | undefined;
  intensity: () => number;
  speed: () => number;
  low: () => boolean;
  /** Fallback colours for the aura mesh. */
  auraPalette: () => number[];
  dark?: () => boolean;
}

/** Draws scene `id` on the canvas until the returned stop function is called. */
export function scene(canvas: HTMLCanvasElement, opts: SceneOpts): () => void {
  if (opts.id === "aura") {
    return mesh(canvas, { palette: opts.auraPalette, dark: opts.dark ?? (() => true), intensity: opts.intensity, scale: 0.4 });
  }
  const gl = canvas.getContext("webgl2", { antialias: false, alpha: false, preserveDrawingBuffer: false });
  if (!gl) return () => {};
  let prog: WebGLProgram;
  try {
    prog = program(gl, VERT, HEAD + SCENES[opts.id] + MAIN);
  } catch (e) {
    console.error("scene", opts.id, e);
    return () => {};
  }
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  gl.useProgram(prog);
  const loc = gl.getAttribLocation(prog, "p");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  const u = (n: string) => gl.getUniformLocation(prog, n);
  const U = {
    t: u("uT"), z: u("uZ"), lvl: u("uLvl"), bass: u("uBass"), mid: u("uMid"), high: u("uHigh"),
    beat: u("uBeat"), amt: u("uAmt"), res: u("uRes"), c: u("uC"), s: u("uS"),
  };
  // Base travel speed per scene (distance units per second), and how much loudness adds.
  const travel: Record<string, [number, number]> = { space: [0.012, 0.03], silk: [0.02, 0.04], aurora: [0.02, 0.04], depth: [0.03, 0.06], smoke: [0.02, 0.04], void: [0.01, 0.02] };
  const [base, loud] = travel[opts.id] ?? [0.03, 0.1];
  // Stars need real pixels; soft scenes look identical at a fraction of the resolution.
  const res = opts.id === "space" || opts.id === "aurora" ? [0.8, 0.55] : [0.45, 0.32];

  const cur = new Float32Array(vividPalette(opts.palette()));
  const spec = new Float32Array(16);
  let t = Math.random() * 100, z = Math.random(), last = performance.now(), raf = 0;
  const loop = (now: number) => {
    raf = requestAnimationFrame(loop);
    const scale = opts.low() ? res[1] : res[0];
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.max(1, Math.round(canvas.clientWidth * dpr * scale));
    const h = Math.max(1, Math.round(canvas.clientHeight * dpr * scale));
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; gl.viewport(0, 0, w, h); }
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    const amt = opts.intensity();
    const sp = opts.speed();
    t += dt * sp * (0.7 + frame.lvl * 1.2 * amt);
    z = (z + dt * sp * (base + frame.lvl * loud * amt + frame.beat * loud * 0.6 * amt)) % 1000;
    const target = vividPalette(opts.palette());
    for (let i = 0; i < 12; i++) cur[i] += (target[i] - cur[i]) * Math.min(1, dt * 2);
    const b = frame.b;
    for (let i = 0; i < 16; i++) spec[i] = ((b[i * 3] ?? 0) + (b[i * 3 + 1] ?? 0) + (b[i * 3 + 2] ?? 0)) / (3 * 255); // bytes → 0–1
    gl.uniform1f(U.t, t);
    gl.uniform1f(U.z, z);
    gl.uniform1f(U.lvl, frame.lvl);
    gl.uniform1f(U.bass, frame.bass);
    gl.uniform1f(U.mid, frame.mid);
    gl.uniform1f(U.high, frame.high);
    gl.uniform1f(U.beat, frame.beat);
    gl.uniform1f(U.amt, amt);
    gl.uniform2f(U.res, w, h);
    gl.uniform3fv(U.c, cur);
    gl.uniform1fv(U.s, spec);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };
  raf = requestAnimationFrame(loop);
  return () => cancelAnimationFrame(raf);
}

export const SCENE_LIST: { id: SceneId; name: string; note: string }[] = [
  { id: "aura", name: "Аура", note: "Живой градиент из обложки" },
  { id: "space", name: "Космос", note: "Туманность цвета обложки и медленный полёт сквозь звёзды" },
  { id: "silk", name: "Шёлк", note: "Ленты света, которые колышутся от баса" },
  { id: "aurora", name: "Сияние", note: "Северное сияние, лучи подсвечивает спектр" },
  { id: "depth", name: "Огни", note: "Сцена не в фокусе: мягкие огни, как сквозь объектив" },
  { id: "smoke", name: "Дым", note: "Дым, подсвеченный снизу в такт басу" },
  { id: "void", name: "Пустота", note: "Только слова и дыхание цвета" },
];

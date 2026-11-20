// Audio-reactive flowing gradient. Domain-warped noise mixes four cover colours;
// loudness speeds up the flow, bass breathes the brightness, beats add a soft bloom.
// Film grain is added in-shader so 8-bit gradients never band.

import { frame } from "../store.svelte";

const VERT = `#version 300 es
in vec2 p; out vec2 uv;
void main(){ uv = p * 0.5 + 0.5; gl_Position = vec4(p, 0.0, 1.0); }`;

const FRAG = `#version 300 es
precision highp float;
in vec2 uv; out vec4 o;
uniform float uT, uLvl, uBass, uBeat, uDark, uAmt;
uniform vec2 uRes;
uniform vec3 uC[4];

float hash(vec2 p){ p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
float noise(vec2 p){
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1,0)), u.x), mix(hash(i + vec2(0,1)), hash(i + vec2(1,1)), u.x), u.y);
}
float fbm(vec2 p){ float v = 0.0, a = 0.5; for(int i = 0; i < 5; i++){ v += a * noise(p); p = p * 2.03 + 17.1; a *= 0.5; } return v; }

void main(){
  vec2 st = uv; st.x *= uRes.x / uRes.y;
  float t = uT * 0.05;
  vec2 q = vec2(fbm(st * 1.4 + t), fbm(st * 1.4 - t + 3.7));
  vec2 r = vec2(fbm(st * 1.2 + 2.2 * q + vec2(1.7, 9.2) + t * 1.3), fbm(st * 1.2 + 2.2 * q + vec2(8.3, 2.8) - t));
  float f = fbm(st + r * (1.6 + uBass * 0.6 * uAmt));

  vec3 col = mix(uC[0], uC[1], smoothstep(0.15, 0.85, f));
  col = mix(col, uC[2], smoothstep(0.35, 0.95, length(q)));
  col = mix(col, uC[3], smoothstep(0.55, 1.0, r.x) * 0.6);

  // Keep it as a background: tame saturation and push towards the theme's base.
  float l = dot(col, vec3(0.299, 0.587, 0.114));
  col = mix(vec3(l), col, 0.9);
  if (uDark > 0.5) {
    // Keep the hue, cap the brightness: a yellow or white cover must still give a field
    // white text can sit on. Luma is compressed into ~0.05–0.2 instead of scaled.
    float L = max(dot(col, vec3(0.299, 0.587, 0.114)), 1e-3);
    float target = 0.07 + 0.19 * smoothstep(0.0, 1.0, L);
    col = col * (target / L);
    col = mix(vec3(dot(col, vec3(0.299, 0.587, 0.114))), col, 1.4); // win back saturation
    col = max(col, 0.0);
  } else {
    col = mix(col, col * 1.08, 0.55);
  }
  col += (uBass * 0.05 + uBeat * 0.04) * uAmt * (uDark > 0.5 ? 0.7 : 1.0);

  // Vignette
  vec2 d = uv - 0.5;
  col *= 1.0 - dot(d, d) * (uDark > 0.5 ? 0.9 : 0.3);
  // Grain
  col += (hash(uv * uRes + fract(uT)) - 0.5) * 0.025;
  o = vec4(col, 1.0);
}`;

function compile(gl: WebGL2RenderingContext, type: number, src: string) {
  const s = gl.createShader(type)!;
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? "shader");
  return s;
}

export function program(gl: WebGL2RenderingContext, vs: string, fs: string) {
  const p = gl.createProgram()!;
  gl.attachShader(p, compile(gl, gl.VERTEX_SHADER, vs));
  gl.attachShader(p, compile(gl, gl.FRAGMENT_SHADER, fs));
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? "link");
  return p;
}

export interface MeshOpts {
  palette: () => number[]; // 12 floats
  dark: () => boolean;
  intensity: () => number;
  /** Render scale; the gradient is soft so a fraction of native resolution looks identical. */
  scale?: number;
}

export function mesh(canvas: HTMLCanvasElement, opts: MeshOpts) {
  const gl = canvas.getContext("webgl2", { antialias: false, alpha: false, preserveDrawingBuffer: false });
  if (!gl) return () => {};
  const prog = program(gl, VERT, FRAG);
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "p");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  gl.useProgram(prog);
  const u = (n: string) => gl.getUniformLocation(prog, n);
  const uT = u("uT"), uRes = u("uRes"), uC = u("uC"), uLvl = u("uLvl"), uBass = u("uBass"), uBeat = u("uBeat"), uDark = u("uDark"), uAmt = u("uAmt");

  const cur = new Float32Array(opts.palette());
  let raf = 0;
  let t = Math.random() * 100;
  let last = performance.now();
  const scale = opts.scale ?? 0.5;

  const loop = (now: number) => {
    raf = requestAnimationFrame(loop);
    const w = Math.max(1, Math.round(canvas.clientWidth * scale));
    const h = Math.max(1, Math.round(canvas.clientHeight * scale));
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; gl.viewport(0, 0, w, h); }
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    const amt = opts.intensity();
    t += dt * (0.6 + frame.lvl * 2.2 * amt);
    // Ease palette changes so track switches cross-fade instead of cutting.
    const target = opts.palette();
    for (let i = 0; i < 12; i++) cur[i] += (target[i] - cur[i]) * Math.min(1, dt * 2.5);
    gl.uniform1f(uT, t);
    gl.uniform2f(uRes, w, h);
    gl.uniform3fv(uC, cur);
    gl.uniform1f(uLvl, frame.lvl);
    gl.uniform1f(uBass, frame.bass);
    gl.uniform1f(uBeat, frame.beat);
    gl.uniform1f(uDark, opts.dark() ? 1 : 0);
    gl.uniform1f(uAmt, amt);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };
  raf = requestAnimationFrame(loop);
  return () => {
    // No loseContext(): the same canvas may be initialised again, and a lost context
    // cannot be reused. The browser frees it with the canvas element.
    cancelAnimationFrame(raf);
  };
}

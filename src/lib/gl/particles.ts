// Dust/ember particles that live in a slow curl-ish flow field.
// Bass pushes them outward from a focal point, beats flare their size and brightness.
// Simulated on the CPU (a few hundred points), drawn as additive soft sprites in WebGL2.

import { frame } from "../store.svelte";
import { program } from "./mesh";

const VERT = `#version 300 es
in vec2 aPos; in float aSize; in float aAlpha;
uniform vec2 uRes; uniform float uDpr;
out float vA;
void main(){
  vec2 c = aPos / uRes * 2.0 - 1.0; c.y = -c.y;
  gl_Position = vec4(c, 0.0, 1.0);
  gl_PointSize = aSize * uDpr;
  vA = aAlpha;
}`;
const FRAG = `#version 300 es
precision mediump float;
in float vA; out vec4 o;
uniform vec3 uCol;
void main(){
  vec2 d = gl_PointCoord - 0.5;
  float r = length(d);
  float a = smoothstep(0.5, 0.0, r);
  a = a * a * vA;
  o = vec4(uCol * a, a);
}`;

export interface ParticleOpts {
  count: number;
  color: () => [number, number, number];
  intensity: () => number;
  /** Focal point in 0–1 of the canvas, where bass pushes away from. */
  focus?: () => [number, number];
  enabled: () => boolean;
}

export function particles(canvas: HTMLCanvasElement, opts: ParticleOpts) {
  const gl = canvas.getContext("webgl2", { premultipliedAlpha: true, alpha: true, antialias: false });
  if (!gl) return () => {};
  const prog = program(gl, VERT, FRAG);
  gl.useProgram(prog);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE);

  const n = opts.count;
  const STRIDE = 4; // x, y, size, alpha
  const data = new Float32Array(n * STRIDE);
  const vel = new Float32Array(n * 2);
  const base = new Float32Array(n * 2); // base size, life phase
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  const aPos = gl.getAttribLocation(prog, "aPos");
  const aSize = gl.getAttribLocation(prog, "aSize");
  const aAlpha = gl.getAttribLocation(prog, "aAlpha");
  gl.enableVertexAttribArray(aPos);
  gl.enableVertexAttribArray(aSize);
  gl.enableVertexAttribArray(aAlpha);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 16, 0);
  gl.vertexAttribPointer(aSize, 1, gl.FLOAT, false, 16, 8);
  gl.vertexAttribPointer(aAlpha, 1, gl.FLOAT, false, 16, 12);
  const uRes = gl.getUniformLocation(prog, "uRes");
  const uDpr = gl.getUniformLocation(prog, "uDpr");
  const uCol = gl.getUniformLocation(prog, "uCol");

  let W = 1, H = 1;
  const spawn = (i: number, anywhere: boolean) => {
    data[i * 4] = Math.random() * W;
    data[i * 4 + 1] = anywhere ? Math.random() * H : H + 10;
    vel[i * 2] = (Math.random() - 0.5) * 6;
    vel[i * 2 + 1] = -(4 + Math.random() * 10);
    base[i * 2] = 1.5 + Math.pow(Math.random(), 3) * 7;
    base[i * 2 + 1] = Math.random() * Math.PI * 2;
  };

  let raf = 0, last = performance.now(), t = 0, seeded = false;
  const loop = (now: number) => {
    raf = requestAnimationFrame(loop);
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    W = canvas.clientWidth; H = canvas.clientHeight;
    if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
      canvas.width = Math.round(W * dpr); canvas.height = Math.round(H * dpr);
      gl.viewport(0, 0, canvas.width, canvas.height);
    }
    if (!seeded && W > 1) { for (let i = 0; i < n; i++) spawn(i, true); seeded = true; }
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (!opts.enabled()) return;

    const dt = Math.min(0.05, (now - last) / 1000);
    last = now; t += dt;
    const amt = opts.intensity();
    const [fx, fy] = opts.focus?.() ?? [0.5, 0.5];
    const cx = fx * W, cy = fy * H;
    const push = frame.bass * frame.bass * 220 * amt;
    const flare = frame.beat * amt;

    for (let i = 0; i < n; i++) {
      const o = i * 4;
      let x = data[o], y = data[o + 1];
      // Flow field: cheap rotating sine field gives gentle swirls.
      const ang = Math.sin(x * 0.004 + t * 0.3) + Math.cos(y * 0.005 - t * 0.2);
      let vx = vel[i * 2] + Math.cos(ang * 2.2) * 8;
      let vy = vel[i * 2 + 1] + Math.sin(ang * 2.2) * 5;
      const dx = x - cx, dy = y - cy;
      const d = Math.sqrt(dx * dx + dy * dy) + 40;
      vx += (dx / d) * push * (180 / d);
      vy += (dy / d) * push * (180 / d);
      x += vx * dt; y += vy * dt;
      if (y < -20 || x < -40 || x > W + 40 || y > H + 40) { spawn(i, false); continue; }
      data[o] = x; data[o + 1] = y;
      const tw = 0.55 + 0.45 * Math.sin(t * 1.3 + base[i * 2 + 1]);
      data[o + 2] = base[i * 2] * (1 + flare * 0.9);
      data[o + 3] = (0.10 + 0.32 * tw) * (0.55 + frame.lvl * 0.7 + flare * 0.5) * Math.min(1, amt + 0.2);
    }
    const c = opts.color();
    gl.uniform2f(uRes, W, H);
    gl.uniform1f(uDpr, dpr);
    gl.uniform3f(uCol, c[0], c[1], c[2]);
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW);
    gl.drawArrays(gl.POINTS, 0, n);
  };
  raf = requestAnimationFrame(loop);
  return () => {
    // No loseContext(): the same canvas may be initialised again, and a lost context
    // cannot be reused. The browser frees it with the canvas element.
    cancelAnimationFrame(raf);
  };
}

"use client";

import React, { useEffect, useRef, useState } from "react";

export interface OrganicGrainyBackgroundProps {
  className?: string;
  speed?: number;
  animate?: boolean;
  grainIntensity?: number;
  waveIntensity?: number;
  interactive?: boolean;
  children?: React.ReactNode;
}

const VERTEX_SHADER_SOURCE = `
attribute vec2 position;
varying vec2 vUv;
void main() {
    vUv = position * 0.5 + 0.5;
    gl_Position = vec4(position, 0.0, 1.0);
}
`;

const FRAGMENT_SHADER_SOURCE = `
precision highp float;

uniform vec2 u_resolution;
uniform float u_time;
uniform float u_grain_intensity;
uniform float u_wave_intensity;
uniform vec2 u_mouse;
varying vec2 vUv;

// Palette specification:
// Warm cream: #FFF5D5
const vec3 C_WARM_CREAM    = vec3(1.0, 0.961, 0.835);
// Butter yellow: #FFE7A0
const vec3 C_BUTTER_YELLOW = vec3(1.0, 0.906, 0.627);
// Pale cyan: #8EDCF0
const vec3 C_PALE_CYAN     = vec3(0.557, 0.863, 0.941);
// Powder blue: #A7C8ED
const vec3 C_POWDER_BLUE   = vec3(0.655, 0.784, 0.929);
// Lavender: #B8A8D9
const vec3 C_LAVENDER      = vec3(0.722, 0.659, 0.851);
// Dusty pink: #F1A0B7
const vec3 C_DUSTY_PINK    = vec3(0.945, 0.627, 0.718);
// Coral: #FF806F
const vec3 C_CORAL         = vec3(1.0, 0.502, 0.435);
// Peach: #FFB18C
const vec3 C_PEACH         = vec3(1.0, 0.694, 0.549);
// Hot pink: #F43D7B
const vec3 C_HOT_PINK      = vec3(0.957, 0.239, 0.482);
// Raspberry: #C72C59
const vec3 C_RASPBERRY     = vec3(0.780, 0.173, 0.349);

// Procedural 2D Simplex Noise
vec3 mod289(vec3 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec2 mod289(vec2 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec3 permute(vec3 x) { return mod289(((x * 34.0) + 1.0) * x); }

float snoise(vec2 v) {
    const vec4 C = vec4(0.211324865405187,
                        0.366025403784439,
                       -0.577350269189626,
                        0.024390243902439);
    vec2 i  = floor(v + dot(v, C.yy));
    vec2 x0 = v -   i + dot(i, C.xx);
    vec2 i1 = (x0.x > x0.y) ? vec2(1.0, 0.0) : vec2(0.0, 1.0);
    vec4 x12 = x0.xyxy + C.xxzz;
    x12.xy -= i1;
    i = mod289(i);
    vec3 p = permute(permute(i.y + vec3(0.0, i1.y, 1.0)) + i.x + vec3(0.0, i1.x, 1.0));
    vec3 m = max(0.5 - vec3(dot(x0, x0), dot(x12.xy, x12.xy), dot(x12.zw, x12.zw)), 0.0);
    m = m * m;
    m = m * m;
    vec3 x = 2.0 * fract(p * C.www) - 1.0;
    vec3 h = abs(x) - 0.5;
    vec3 ox = floor(x + 0.5);
    vec3 a0 = x - ox;
    m *= 1.79284291400159 - 0.85373472095314 * (a0 * a0 + h * h);
    vec3 g;
    g.x = a0.x * x0.x + h.x * x0.y;
    g.yz = a0.yz * x12.xz + h.yz * x12.yw;
    return 130.0 * dot(m, g);
}

// Multi-octave Fractal Brownian Motion (FBM) with rotation
float fbm(vec2 p) {
    float f = 0.0;
    float amp = 0.5;
    mat2 rot = mat2(0.80, 0.60, -0.60, 0.80);
    for (int i = 0; i < 4; i++) {
        f += amp * snoise(p);
        p = rot * p * 2.05 + vec2(1.7, 3.2);
        amp *= 0.5;
    }
    return f;
}

// Multi-stage Domain Warping: p' = p + fbm(p + fbm(p))
vec2 domainWarp(vec2 p, float t, float strength) {
    // Stage 1: Broad sweeping fluid currents
    vec2 q = vec2(
        fbm(p + vec2(0.0, 0.0) + vec2(t * 0.035, t * 0.02)),
        fbm(p + vec2(5.2, 1.3) + vec2(-t * 0.025, t * 0.03))
    );

    // Stage 2: Second-order warping (produces asymmetrical, curling wave ribbons)
    vec2 r = vec2(
        fbm(p + 3.0 * q + vec2(1.7, 9.2) + vec2(t * 0.015, -t * 0.02)),
        fbm(p + 3.0 * q + vec2(8.3, 2.8) + vec2(-t * 0.012, t * 0.018))
    );

    // Stage 3: Fine wave undulation
    vec2 s = vec2(
        snoise(p * 2.0 + r * 1.6 + vec2(t * 0.035, -t * 0.025)),
        snoise(p * 2.0 + r * 1.6 + vec2(-t * 0.025, t * 0.035))
    );

    return p + (q * 0.55 + r * 0.35 + s * 0.12) * strength;
}

// Procedural analog film grain with triangular distribution
float filmGrain(vec2 fragCoord, float t) {
    vec2 p = fragCoord;
    float r1 = fract(sin(dot(p, vec2(12.9898, 78.233)) + t * 0.01) * 43758.5453);
    float r2 = fract(sin(dot(p + vec2(1.5, 3.7), vec2(39.345, 93.123)) + t * 0.01) * 23421.631);
    return (r1 + r2 - 1.0);
}

// Analog film response with subtle RGB micro-chroma
vec3 applyFilmGrain(vec3 color, vec2 fragCoord, float t, float intensity) {
    float lum = dot(color, vec3(0.299, 0.587, 0.114));
    
    // Analog film curve: visible across both dark and bright areas, peak texture in midtones
    float response = mix(0.70, 1.25, 1.0 - abs(lum - 0.45));

    float gR = filmGrain(fragCoord, t);
    float gG = filmGrain(fragCoord + vec2(13.1, 7.3), t);
    float gB = filmGrain(fragCoord + vec2(27.4, 19.9), t);

    float monoGrain = (gR + gG + gB) * 0.3333;
    vec3 grainVec = mix(vec3(monoGrain), vec3(gR, gG, gB), 0.22);

    return color + grainVec * (intensity * response);
}

void main() {
    // Screen UV: st.x in [0, 1], st.y in [0, 1] (0 is bottom, 1 is top)
    vec2 st = gl_FragCoord.xy / u_resolution.xy;
    float aspect = u_resolution.x / u_resolution.y;

    // Time scaling for ultra-smooth liquid flow
    float t = u_time;

    // Subtle interactive mouse influence if active
    vec2 mouseOffset = (u_mouse - 0.5) * 0.08;

    // Aspect-normalized coordinates for wave calculations to avoid stretching
    vec2 normCoord = vec2((st.x - 0.5) * aspect + 0.5, st.y);
    vec2 warpDomain = normCoord * 1.6 + mouseOffset;

    // Perform multi-layer domain warping
    vec2 warped = domainWarp(warpDomain, t, u_wave_intensity);

    // Compute spatial coordinates deformed by the warped domain
    vec2 wUv = st + (warped - warpDomain) * 0.38;

    // -------------------------------------------------------------
    // COLOR FIELD SYNTHESIS
    // -------------------------------------------------------------

    // 1. Upper-Left Field: Warm Butter Yellow & Cream
    float distUL = length(vec2(wUv.x - 0.12, wUv.y - 0.98));
    float waveUL = sin(wUv.x * 3.2 - wUv.y * 2.8 + fbm(wUv * 2.5 + vec2(t * 0.02, 0.0)) * 2.2);
    float wYellow = smoothstep(0.85, 0.18, distUL + waveUL * 0.15);
    vec3 yellowCol = mix(C_WARM_CREAM, C_BUTTER_YELLOW, smoothstep(0.12, 0.55, distUL));

    // 2. Upper-Center & Upper-Right: Pale Cyan & Powder Blue
    float cyanBand = (wUv.x * 0.82 + wUv.y * 0.72) - 0.65;
    float waveCyan = cos(wUv.x * 3.8 + wUv.y * 2.6 + fbm(wUv * 2.6 + vec2(3.0, t * 0.025)) * 2.4);
    float wCyan = smoothstep(-0.25, 0.38, cyanBand + waveCyan * 0.22) * smoothstep(0.28, 0.68, wUv.y);
    vec3 blueCol = mix(C_PALE_CYAN, C_POWDER_BLUE, smoothstep(0.45, 0.85, wUv.x));

    // 3. Right & Mid-Right Field: Lavender, Dusty Pink & Vibrant Coral
    float distR = length(vec2(wUv.x - 0.98, wUv.y - 0.48));
    float waveR = sin(wUv.y * 4.5 - wUv.x * 2.2 + fbm(wUv * 2.8 + vec2(5.0, -t * 0.02)) * 2.0);
    float wRight = smoothstep(0.88, 0.22, distR + waveR * 0.18);
    vec3 rightCol = mix(C_LAVENDER, C_DUSTY_PINK, smoothstep(-0.35, 0.35, waveR));
    rightCol = mix(rightCol, C_CORAL, smoothstep(0.68, 0.98, wUv.x));

    // 4. Bottom-Left Field: Raspberry & Hot Pink
    float distBL = length(vec2(wUv.x - 0.15, wUv.y - 0.12));
    float waveBL = sin(wUv.x * 3.6 + wUv.y * 3.2 + fbm(wUv * 2.4 + vec2(7.0, t * 0.018)) * 2.2);
    float wRed = smoothstep(0.82, 0.18, distBL + waveBL * 0.18);
    vec3 redCol = mix(C_HOT_PINK, C_RASPBERRY, smoothstep(0.42, 0.15, wUv.y));

    // 5. Center-Left & Lower-Middle Base: Warm Peach & Coral
    vec3 basePeach = mix(C_PEACH, C_CORAL, smoothstep(0.25, 0.78, wUv.x));

    // -------------------------------------------------------------
    // ORGANIC WAVE COMPOSITION & BLENDING
    // -------------------------------------------------------------
    vec3 col = basePeach;

    // Raspberry & Hot Pink flowing from bottom-left
    col = mix(col, redCol, clamp(wRed, 0.0, 1.0));

    // Pale Cyan & Powder Blue flowing across top-center & top-right
    col = mix(col, blueCol, clamp(wCyan * (1.0 - wRed * 0.7), 0.0, 1.0));

    // Butter Yellow & Warm Cream pouring into top-left
    col = mix(col, yellowCol, clamp(wYellow * (1.0 - wRed * 0.5), 0.0, 1.0));

    // Lavender, Dusty Pink & Coral streaming along the right
    col = mix(col, rightCol, clamp(wRight * (1.0 - wYellow * 0.55), 0.0, 1.0));

    // Secondary flowing wave ribbons weaving between the boundaries
    float ribbonCoral = smoothstep(0.18, 0.0, abs(sin(wUv.x * 3.4 - wUv.y * 2.8 + fbm(wUv * 3.0) * 2.8) - 0.15));
    col = mix(col, C_CORAL, ribbonCoral * 0.32 * smoothstep(0.18, 0.75, wUv.y));

    float ribbonLavender = smoothstep(0.20, 0.0, abs(cos(wUv.x * 2.8 + wUv.y * 3.2 + fbm(wUv * 3.2 + 2.0) * 2.6) - 0.10));
    col = mix(col, C_LAVENDER, ribbonLavender * 0.28 * smoothstep(0.35, 0.85, wUv.x));

    // Subtle micro-luminance variation for painterly analog depth
    float microVariation = snoise(wUv * 8.0 + t * 0.02) * 0.022;
    col += microVariation;

    // Fine analog film grain overlay with 35mm photographic response
    col = applyFilmGrain(col, gl_FragCoord.xy, u_time, u_grain_intensity);

    // Final color clamped to valid display range
    gl_FragColor = vec4(clamp(col, 0.0, 1.0), 1.0);
}
`;

export function OrganicGrainyBackground({
  className = "",
  speed = 0.12,
  animate = true,
  grainIntensity = 0.075,
  waveIntensity = 1.0,
  interactive = false,
  children,
}: OrganicGrainyBackgroundProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [webGlSupported, setWebGlSupported] = useState(true);
  const mousePosRef = useRef({ x: 0.5, y: 0.5 });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    // Check prefers-reduced-motion
    const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    const shouldAnimate = animate && !mediaQuery.matches;

    // Initialize WebGL context
    const gl =
      canvas.getContext("webgl", {
        alpha: false,
        depth: false,
        stencil: false,
        antialias: false,
        powerPreference: "default",
      }) ||
      (canvas.getContext(
        "experimental-webgl"
      ) as WebGLRenderingContext | null);

    if (!gl) {
      setWebGlSupported(false);
      return;
    }

    // Shader compilation utility
    function compileShader(source: string, type: number): WebGLShader | null {
      if (!gl) return null;
      const shader = gl.createShader(type);
      if (!shader) return null;
      gl.shaderSource(shader, source);
      gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        console.error("Shader compilation error:", gl.getShaderInfoLog(shader));
        gl.deleteShader(shader);
        return null;
      }
      return shader;
    }

    const vertexShader = compileShader(VERTEX_SHADER_SOURCE, gl.VERTEX_SHADER);
    const fragmentShader = compileShader(
      FRAGMENT_SHADER_SOURCE,
      gl.FRAGMENT_SHADER
    );

    if (!vertexShader || !fragmentShader) {
      setWebGlSupported(false);
      return;
    }

    const program = gl.createProgram();
    if (!program) {
      setWebGlSupported(false);
      return;
    }

    gl.attachShader(program, vertexShader);
    gl.attachShader(program, fragmentShader);
    gl.linkProgram(program);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      console.error("Program link error:", gl.getProgramInfoLog(program));
      gl.deleteProgram(program);
      setWebGlSupported(false);
      return;
    }

    gl.useProgram(program);

    // Full screen quad buffer
    const positionBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
      gl.STATIC_DRAW
    );

    const positionLocation = gl.getAttribLocation(program, "position");
    gl.enableVertexAttribArray(positionLocation);
    gl.vertexAttribPointer(positionLocation, 2, gl.FLOAT, false, 0, 0);

    // Uniform locations
    const resolutionLocation = gl.getUniformLocation(program, "u_resolution");
    const timeLocation = gl.getUniformLocation(program, "u_time");
    const grainLocation = gl.getUniformLocation(program, "u_grain_intensity");
    const waveLocation = gl.getUniformLocation(program, "u_wave_intensity");
    const mouseLocation = gl.getUniformLocation(program, "u_mouse");

    let animationFrameId: number;
    let startTime = performance.now();
    let isContextLost = false;

    // Resize handling with devicePixelRatio cap for crispness & performance
    function handleResize() {
      if (!canvas || !gl || isContextLost) return;
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 2.0);
      const width = Math.max(1, Math.round(rect.width * dpr));
      const height = Math.max(1, Math.round(rect.height * dpr));

      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
        gl.viewport(0, 0, width, height);
      }
    }

    const resizeObserver = new ResizeObserver(() => {
      handleResize();
      if (!shouldAnimate) {
        render(startTime);
      }
    });

    if (containerRef.current) {
      resizeObserver.observe(containerRef.current);
    }
    handleResize();

    // Mouse movement listener if interactive
    function onMouseMove(e: MouseEvent) {
      if (!canvas) return;
      const rect = canvas.getBoundingClientRect();
      mousePosRef.current = {
        x: (e.clientX - rect.left) / rect.width,
        y: 1.0 - (e.clientY - rect.top) / rect.height,
      };
      if (!shouldAnimate) {
        render(performance.now());
      }
    }

    if (interactive) {
      window.addEventListener("mousemove", onMouseMove, { passive: true });
    }

    // Render loop
    function render(currentTime: number) {
      if (!gl || !canvas || isContextLost) return;

      const elapsed = (currentTime - startTime) * 0.001 * speed;

      gl.useProgram(program);
      gl.uniform2f(resolutionLocation, canvas.width, canvas.height);
      gl.uniform1f(timeLocation, elapsed);
      gl.uniform1f(grainLocation, grainIntensity);
      gl.uniform1f(waveLocation, waveIntensity);
      gl.uniform2f(
        mouseLocation,
        mousePosRef.current.x,
        mousePosRef.current.y
      );

      gl.drawArrays(gl.TRIANGLES, 0, 6);

      if (shouldAnimate) {
        animationFrameId = requestAnimationFrame(render);
      }
    }

    // Trigger initial render
    render(startTime);

    // WebGL Context loss & restore handlers
    function handleContextLost(e: Event) {
      e.preventDefault();
      isContextLost = true;
      if (animationFrameId) cancelAnimationFrame(animationFrameId);
    }

    function handleContextRestored() {
      isContextLost = false;
      handleResize();
      render(performance.now());
    }

    canvas.addEventListener("webglcontextlost", handleContextLost, false);
    canvas.addEventListener(
      "webglcontextrestored",
      handleContextRestored,
      false
    );

    return () => {
      if (animationFrameId) {
        cancelAnimationFrame(animationFrameId);
      }
      resizeObserver.disconnect();
      if (interactive) {
        window.removeEventListener("mousemove", onMouseMove);
      }
      canvas.removeEventListener("webglcontextlost", handleContextLost);
      canvas.removeEventListener(
        "webglcontextrestored",
        handleContextRestored
      );

      // Clean up WebGL resources
      if (gl) {
        gl.deleteBuffer(positionBuffer);
        gl.deleteProgram(program);
        gl.deleteShader(vertexShader);
        gl.deleteShader(fragmentShader);
      }
    };
  }, [speed, animate, grainIntensity, waveIntensity, interactive]);

  return (
    <div
      ref={containerRef}
      className={`relative w-full h-full overflow-hidden ${className}`}
    >
      {webGlSupported ? (
        <canvas
          ref={canvasRef}
          aria-hidden="true"
          className="absolute inset-0 w-full h-full pointer-events-none block"
          style={{ willChange: animate ? "contents" : "auto" }}
        />
      ) : (
        /* Graceful CSS & SVG Grain Fallback if WebGL is unavailable */
        <div className="grainy-gradient absolute inset-0 w-full h-full pointer-events-none" />
      )}
      {/* Foreground content container */}
      {children && (
        <div className="relative z-10 w-full h-full">{children}</div>
      )}
    </div>
  );
}

export default OrganicGrainyBackground;

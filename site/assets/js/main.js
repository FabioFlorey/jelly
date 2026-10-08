    (() => {
      const hero = document.getElementById("hero");
      const canvas = document.getElementById("heroLiquid");
      if (!hero || !canvas) return;

      const gl = canvas.getContext("webgl", {
        alpha: false,
        antialias: false,
        depth: false,
        stencil: false,
        powerPreference: "high-performance"
      });
      if (!gl) return;

      const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      let raf = 0;
      let active = true;
      let lastFrame = 0;

      const vertexSource = `
        attribute vec2 aPosition;
        varying vec2 vUv;

        void main() {
          vUv = aPosition * .5 + .5;
          gl_Position = vec4(aPosition, 0.0, 1.0);
        }
      `;

      const fragmentSource = `
        // Pixel-coordinate noise needs high precision: mediump can quantize
        // large fragment coordinates into visible vertical bands.
        precision highp float;

        uniform vec2 uResolution;
        uniform float uTime;
        varying vec2 vUv;

        float hash21(vec2 p) {
          p = fract(p * vec2(123.34, 456.21));
          p += dot(p, p + 45.32);
          return fract(p.x * p.y);
        }

        float valueNoise(vec2 p) {
          vec2 i = floor(p);
          vec2 f = fract(p);
          f = f * f * (3.0 - 2.0 * f);

          float a = hash21(i);
          float b = hash21(i + vec2(1.0, 0.0));
          float c = hash21(i + vec2(0.0, 1.0));
          float d = hash21(i + vec2(1.0, 1.0));

          return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
        }

        float fbm(vec2 p) {
          float sum = 0.0;
          float amp = .5;
          for (int i = 0; i < 3; i++) {
            sum += valueNoise(p) * amp;
            p = mat2(1.62, 1.18, -1.18, 1.62) * p + 7.13;
            amp *= .5;
          }
          return sum;
        }

        float blob(vec2 uv, vec2 center, float radius) {
          float d = length(uv - center);
          return 1.0 - smoothstep(radius * .12, radius, d);
        }

        void main() {
          vec2 uv = vUv;
          float aspect = uResolution.x / max(uResolution.y, 1.0);
          vec2 p = vec2((uv.x - .5) * aspect, uv.y - .5);

          float t = uTime;

          vec2 q = vec2(
            fbm(p * 2.15 + vec2(t * .055, -t * .037)),
            fbm(p * 2.15 + vec2(-t * .043, t * .061) + 17.0)
          );

          vec2 r = vec2(
            fbm(p * 2.8 + q * 1.75 + vec2(t * .029, t * .041)),
            fbm(p * 2.8 + q * 1.75 + vec2(-t * .035, t * .027) + 31.0)
          );

          vec2 warped = p + (q - .5) * .23 + (r - .5) * .13;
          warped += vec2(sin(t * .18), cos(t * .14)) * .025;

          vec2 c1 = vec2(-.50 + sin(t * .31) * .26,  .10 + cos(t * .24) * .25);
          vec2 c2 = vec2( .05 + cos(t * .27) * .34, -.18 + sin(t * .33) * .23);
          vec2 c3 = vec2( .52 + sin(t * .23) * .26,  .20 + cos(t * .29) * .27);
          vec2 c4 = vec2(-.10 + cos(t * .20) * .42,  .46 + sin(t * .26) * .21);
          vec2 c5 = vec2( .38 + cos(t * .36) * .28, -.42 + sin(t * .23) * .18);

          float b1 = blob(warped, c1, .76);
          float b2 = blob(warped, c2, .68);
          float b3 = blob(warped, c3, .82);
          float b4 = blob(warped, c4, .72);
          float b5 = blob(warped, c5, .66);

          vec3 base = vec3(.035, .031, .018);
          vec3 honey = vec3(1.0, .757, .027);
          vec3 amber = vec3(1.0, .40, .015);
          vec3 gold = vec3(1.0, .88, .34);
          vec3 bronze = vec3(.48, .27, .015);

          vec3 color = base;
          color += honey * b1 * .96;
          color += amber * b2 * .70;
          color += gold * b3 * .56;
          color += bronze * b4 * .78;
          color += honey * b5 * .52;

          float folds = fbm(warped * 5.0 + r * 2.0 - t * .06);
          color *= .76 + folds * .50;

          float sheen = pow(smoothstep(.48, .82, folds), 5.0);
          color += vec3(1.0, .73, .12) * sheen * .09;

          float grain = hash21(gl_FragCoord.xy + floor(t * 24.0) * vec2(17.0, 29.0)) - .5;
          color += grain * .038;

          vec2 edge = vUv * (1.0 - vUv);
          float vignette = pow(clamp(edge.x * edge.y * 18.0, 0.0, 1.0), .22);
          color *= mix(.52, 1.0, vignette);

          color = pow(max(color, 0.0), vec3(.92));
          gl_FragColor = vec4(color, 1.0);
        }
      `;

      function shader(type, source) {
        const value = gl.createShader(type);
        gl.shaderSource(value, source);
        gl.compileShader(value);
        if (!gl.getShaderParameter(value, gl.COMPILE_STATUS)) {
          console.error(gl.getShaderInfoLog(value));
          gl.deleteShader(value);
          return null;
        }
        return value;
      }

      const vertex = shader(gl.VERTEX_SHADER, vertexSource);
      const fragment = shader(gl.FRAGMENT_SHADER, fragmentSource);
      if (!vertex || !fragment) return;

      const program = gl.createProgram();
      gl.attachShader(program, vertex);
      gl.attachShader(program, fragment);
      gl.linkProgram(program);
      gl.deleteShader(vertex);
      gl.deleteShader(fragment);

      if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
        console.error(gl.getProgramInfoLog(program));
        gl.deleteProgram(program);
        return;
      }

      gl.useProgram(program);

      const buffer = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.bufferData(
        gl.ARRAY_BUFFER,
        new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
        gl.STATIC_DRAW
      );

      const position = gl.getAttribLocation(program, "aPosition");
      gl.enableVertexAttribArray(position);
      gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);

      const resolution = gl.getUniformLocation(program, "uResolution");
      const timeUniform = gl.getUniformLocation(program, "uTime");

      function resize() {
        const rect = hero.getBoundingClientRect();
        const dpr = Math.min(window.devicePixelRatio || 1, 1);
        const width = Math.max(1, Math.round(rect.width * dpr));
        const height = Math.max(1, Math.round(rect.height * dpr));

        if (canvas.width !== width || canvas.height !== height) {
          canvas.width = width;
          canvas.height = height;
          canvas.style.width = rect.width + "px";
          canvas.style.height = rect.height + "px";
          gl.viewport(0, 0, width, height);
        }
      }

      function draw(ms) {
        if (!active && !reducedMotion) return;

        if (!reducedMotion && ms - lastFrame < 33) {
          raf = requestAnimationFrame(draw);
          return;
        }
        lastFrame = ms;

        gl.uniform2f(resolution, canvas.width, canvas.height);
        gl.uniform1f(timeUniform, reducedMotion ? 0.0 : ms * .001);
        gl.drawArrays(gl.TRIANGLES, 0, 6);

        if (!reducedMotion) raf = requestAnimationFrame(draw);
      }

      const observer = new ResizeObserver(() => {
        resize();
        if (reducedMotion) draw(0);
      });
      observer.observe(hero);

      const visibility = new IntersectionObserver(entries => {
        active = entries[0]?.isIntersecting ?? true;
        if (active && !reducedMotion) {
          cancelAnimationFrame(raf);
          lastFrame = 0;
          raf = requestAnimationFrame(draw);
        } else if (!active) {
          cancelAnimationFrame(raf);
        }
      }, { threshold: 0.01 });
      visibility.observe(hero);

      resize();
      if (reducedMotion) {
        draw(0);
      } else {
        raf = requestAnimationFrame(draw);
      }

      window.addEventListener("pagehide", () => {
        cancelAnimationFrame(raf);
        observer.disconnect();
        visibility.disconnect();
      }, { once: true });
    })();

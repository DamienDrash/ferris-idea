# ADR-0028: Rendering of the web client

- **Status:** proposed, not decided · **Version:** 1 · **Date:** 2026-11-09
- **Related:** [ADR-0003](0003-ui-toolkit.md) (UI toolkit),
  [ADR-0019](0019-accessibility-path-for-gpui.md) (accessibility),
  [ADR-0025](0025-network-security-for-remote-connections.md),
  [ADR-0027](0027-ui-driver-and-semantic-tree.md),
  [SPEC-0325](../specifications/SPEC-0325-wss-endpoint.md) (transport),
  [`REMOTE_AND_MOBILE.md`](../architecture/REMOTE_AND_MOBILE.md) section M18, step 2
- **Evidence:** branch `spike/m18-gpui-web`, folder `spikes/gpui-web/`. An experiment; it pulls the
  upstream GPUI and is not to be merged.

## Question

Can the GPUI-based window run in a browser (WebGPU or WebGL2 through WASM), or does M18 need a
separate web client?

## What was built

The smallest window that exercises the path: upstream GPUI at the commit of draft PR #277
(`c7bea343`), its `gpui_web` platform in single-threaded mode, one embedded font, one text line that
follows typing, and a grid of 2000 labelled cells as a stand-in for editor density. An HTML page
measures the first render and the time from key event to the next frame, and takes `?backend=webgpu`
or `?backend=webgl2`. The IDE itself was not ported.

## Measured

| Measure | Result | How |
|---|---|---|
| Compiles for `wasm32-unknown-unknown` on stable Rust 1.98.1 | yes, single-threaded | real build |
| `.wasm` after `wasm-bindgen` | 8.98 MB (2.76 MB gzip -9) | real build, release, `opt-level=z`, fat LTO |
| `.wasm` after `wasm-opt -Oz` | 5.91 MB (2.39 MB gzip -9) | real build; includes a 194 KB font |
| Release build with LTO, dependencies partly cached | 2 min 28 s | one run, shared machine |
| JS glue | 157 KB (23 KB gzip -9) | real build |
| First frame | 160 ms with WebGPU | 2026-11-09, Claude desktop app browser pane (Chromium 152), one load, page 246×844, machine loaded |
| Key to next frame | WebGPU: median 13.9 ms, p95 26.6 ms (50 keys); `?backend=webgl2`: median 13.6 ms, p95 26.2 ms (48 keys) | same run, real key events one after another; a pasted text produced no key events |
| WebGL2 fallback | page drew and took keys with `?backend=webgl2`; first frame 3142 ms | the log did not show which backend was chosen (WebGPU was still exposed), so this is not proof that WebGL2 ran; the 3 s may include the tab waiting in the background |
| IME German/Japanese/Chinese | **not measured** | `gpui_web` has an `ime_mirror` module; never run |
| VoiceOver, NVDA | **not measured** | needs a person and the ADR-0027 tree |
| Memory with 50 open files | **not measured** | needs the real editor |

The spike has **not been run in any browser**. No statement here says that a frame was ever drawn.
These numbers are a floor: the IDE has far more code than this window, and the bundle will grow.
Brotli was not available on the build machine.

Multi-threaded rendering, which upstream's example uses, needs nightly Rust and `build-std`; a
product on stable Rust would stay single-threaded on this path or take on a nightly toolchain.

## Proposed decision

Keep the M18 plan's hybrid as the working assumption, **conditional on the browser run**: GPUI on
`wasm32` draws, a thin DOM layer provides input method handling, clipboard, an ARIA mirror and menus.
Reasons: the path compiles on stable Rust with no change to GPUI, and about 2.4 MB compressed for
the toolkit is within reach of a web IDE; a separate client would be a second UI codebase.

Proposed abort limits, for the Project Owner to confirm. If any fails, the fallback of the M18 plan
applies: a lean TypeScript client, with the full interface staying in the desktop app.

- First frame at most 3 s on a mid-range laptop, cold, from the server's own origin.
- Key to next frame: median at most 16 ms, 95th percentile at most 33 ms.
- Compressed bundle of the real client at most 15 MB.
- WebGL2 fallback draws correctly with the same code.
- German dead keys, Japanese and Chinese composition work.
- VoiceOver and NVDA can read the editor and project tree. This is the most likely failure and
  decides the approach alone: canvas drawing without a working ARIA mirror is not acceptable.

## Consequences

- Moving to the upstream GPUI (PR #277) becomes a precondition of the GPUI path: `crates/ui` uses
  the crates.io GPUI 0.2.2, which has no web platform.
- `idecraft serve` must then serve the bundle itself (M18.1), which SPEC-0325 does not yet do.
- The CSP of SPEC-0325 behaviour 10 has to allow `wasm-unsafe-eval` for a served client.

## Alternatives

- **Separate TypeScript client now:** certain to work in a browser and accessible by construction,
  but a second UI codebase for an IntelliJ-sized interface. Remains the fallback.
- **Pixel streaming, Theia or OpenVSCode front ends:** already rejected in the M18 plan.

# The laptop model on an integrated GPU — slower than the CPU, and how `gpu = "auto"` should avoid it (2026-10-23)

> **Status: adopted 2026-10-23** (`docs/context/decisions.md`, *an integrated GPU is not "a GPU" for
> `gpu = "auto"`*; the code is `crates/fm-agent-run/src/gpu.rs`). It changes no contract item
> (`contract.md` does not name the `gpu` key, and its three values keep their names).

**The owner:** *"write up the iGPU fallback fix in Myrme"*.

## 1. What happened

- A laptop with a discrete GPU and an integrated one lost the discrete GPU to a driver fault, so
  Vulkan listed **only the integrated GPU**. The same happens on any laptop whose discrete GPU is
  missing, disabled or not yet supported by its driver.
- `gpu = "auto"` passes `-ngl 99` (`Manifest::gpu_layers`), and llama.cpp offloads to whatever Vulkan
  device it finds. So the 4B laptop model moved, silently, onto the integrated GPU.
- It wrote at **~5 tok/s**, with the CPU busy and the fans at full speed. The weights lived in system
  memory shared with the integrated GPU, so the memory pressure stayed too.
- Nothing said so. The only trace was the runtime's own line in the agent log:
  `ggml_vulkan: 0 = <integrated GPU> … | uma: 1 | …`.

## 2. The measurement

One laptop, idle, on AC, with an Intel integrated GPU; formicaria's shipped runtime (`runtime-llg-3`, llama.cpp b10076, Vulkan build);
`llama-bench -m Qwen3VL-4B-Instruct-Q4_K_M.gguf -p 512 -n 64 -r 2 -t 8`, run one after the other:

| Device | Reading (pp512, tok/s) | Writing (tg64, tok/s) |
|---|---|---|
| Integrated GPU (`-ngl 99`) | 80.1 ± 0.1 | **5.7 ± 0.1** |
| CPU only (`-dev none`) | 94.8 ± 1.2 | **14.4 ± 0.0** |
| *A laptop discrete GPU, for reference (speed study §1, 2026-10-19)* | *1575* | *43* |

- **On this integrated GPU the model writes 2.5× slower and reads 15% slower than on the CPU.** Writing
  is where the methods spend their time (speed study §1), so the fallback costs the most where it hurts.
- **The phone showed the same thing:** its Mali GPU reads and writes slower than its CPU (speed study §2.4).
  Both integrated GPUs measured so far lose to their CPU under llama.cpp's Vulkan backend.
- **Not measured:** a stronger integrated GPU (a recent AMD APU, a newer Intel Arc iGPU). One of those
  could beat its CPU, which is why the fix below leaves a way to force the GPU.

## 3. The fix (recommended)

**`gpu = "auto"` means "a discrete GPU when there is one, else the CPU".** An integrated GPU is used
only under `gpu = "on"`.

1. **Detect before launch, with the runtime's own probe.** `llama-bench --list-devices` (in every
   desktop runtime archive Myrme stages) prints one line per Vulkan device, with the flag that
   matters:
   `ggml_vulkan: 0 = <name> (<driver>) | uma: 1 | fp16: 1 | …`.
   `uma: 1` is a GPU that shares system memory, i.e. an integrated one.
   (`llama-server --list-devices` prints only names and memory in this build, not the flag.)
2. **Decide in `agent_serve`**, beside `gpu_layers`:
   - at least one device with `uma: 0` → today's launch (`-ngl 99`); llama.cpp already prefers it;
   - every device `uma: 1` → launch with **`--device none`** (CPU only). Not `-ngl 0`: the speed study
     (§1, *Correction*) found that a Vulkan runtime with no layers offloaded still sends large batches
     to the GPU;
   - no Vulkan device, or no runtime GPU build → unchanged (already the CPU).
3. **Say it once in the agent log**: `integrated GPU only — running on the CPU, which is faster here
   (gpu = "on" forces the GPU)`. A slow assistant should leave a reason a person can find.
4. **If the probe fails or its output cannot be read, keep today's behaviour.** A machine with a working
   discrete GPU must never lose it to a parsing mistake.
5. **macOS is out of scope.** Its runtime is the Metal build, and Apple GPUs are integrated but fast;
   nothing there changes.

**Why parse a log line, and what keeps it honest.** The flag is not in any structured output of the
pinned build. A test against the pinned runtime's `llama-bench --list-devices` (skipped where the
runtime is not fetched, like the other live tests) pins the line's shape, so a runtime bump that
changes it fails there instead of silently falling back.

**Considered and not recommended:**
- **`GGML_VK_VISIBLE_DEVICES`** to hide the integrated device: it needs the device index, which
  still comes from the same probe, and it leaves the "no discrete GPU at all" case to llama.cpp.
- **Detecting from the OS** (`/sys/class/drm`, PCI vendor IDs): integrated and discrete AMD GPUs share
  a vendor, and Windows needs a second path. llama.cpp's own report is the same answer on every
  platform it runs.
- **A short self-benchmark at first start**: it would also cover the strong-iGPU case, but it costs a
  model load and a minute of a person's first run, for a case not yet measured.

## 4. What it changes

| | Before | After |
|---|---|---|
| Discrete GPU present | offloads | offloads (unchanged) |
| Only an integrated GPU (incl. a discrete GPU that dropped out) | offloads to it, ~5 tok/s in §2 | **CPU, ~14 tok/s in §2**, one log line |
| No GPU / CPU runtime | CPU | CPU (unchanged) |
| `gpu = "on"` / `"off"` | always offload / CPU | unchanged |

**Left open:** one measurement on a strong integrated GPU, to know whether `auto` should ever choose
one; and `whisper-server`, which also offloads to an integrated GPU and was not measured.

# Action Guide Live FFmpeg Feasibility Spike - Findings

## Status

- Lifecycle: retained-reference
- Decision owner: Native Action Guide Motion Recording design Gate 0
- Started: 2026-07-30
- Last updated: 2026-07-30

## Decision

Determine whether Rollshot's managed FFmpeg can encode a 1920×1080, 30 fps,
silent, desktop-like RGBA Action Guide stream in real time without blocking the
capture producer or growing Rollshot memory without bound.

## Environment

### Linux (runtime/hardware)

- OS: Linux cachyos-x8664 7.1.5-1-cachyos #1 SMP PREEMPT_DYNAMIC Sun, 26 Jul 2026 08:59:50 +0000 x86_64 GNU/Linux
- CPU: AMD Ryzen 7 255 w/ Radeon 780M Graphics (HawkPoint1)
- Rust: rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 (c980f4866 2026-06-30)
- FFmpeg: ffmpeg version n8.1.2
- ffprobe: ffprobe version n8.1.2

Command:

```
cargo run --release --manifest-path spikes/action-guide-live-ffmpeg/Cargo.toml -- \
  --ffmpeg $ROLLSHOT_FFMPEG --ffprobe $ROLLSHOT_FFPROBE \
  --output spikes/action-guide-live-ffmpeg/reports/linux-10m.mp4 \
  --report spikes/action-guide-live-ffmpeg/reports/linux-10m.json \
  --width 1920 --height 1080 --fps 30 --duration-secs 600 --queue-capacity 2
```

Exit status: 2 (hard gate failure)

### macOS (runtime/hardware)

UNTESTED — Linux hard gate failed first; macOS run not dispatched.

## Risk Results

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| Producer blocking | hard | runtime/hardware | **FAIL** | p99 offer latency 1557 µs > 1000 µs gate; reports/linux-10m.json |
| Persistent queue saturation | hard | runtime/hardware | **PASS** | 0 saturated windows out of 120; reports/linux-10m.json |
| Self memory growth | hard | runtime/hardware | **PASS** | peak-to-trough 1 MiB (≤ 64 MiB), slope 0.13 MiB/min (≤ 1 MiB/min); reports/linux-10m.json |
| Timeline fidelity | hard | runtime/hardware | **PASS** | duration delta 0 ms (≤ 34 ms); reports/linux-10m.json |
| Media contract | hard | runtime/hardware | **PASS** | H.264, 1920x1080, 30/1, 0 audio streams; reports/linux-10m.json |
| Atomic cleanup | hard | automated/runtime | **PASS** | encoder exit status 0, output written atomically; reports/linux-10m.json |

### macOS gates

| Risk | Gate | Evidence | Result | Notes / artifacts |
|---|---|---|---|---|
| Producer blocking | hard | runtime/hardware | UNTESTED | Linux NO-GO stopped dispatch |
| Persistent queue saturation | hard | runtime/hardware | UNTESTED | Linux NO-GO stopped dispatch |
| Self memory growth | hard | runtime/hardware | UNTESTED | Linux NO-GO stopped dispatch |
| Timeline fidelity | hard | runtime/hardware | UNTESTED | Linux NO-GO stopped dispatch |
| Media contract | hard | runtime/hardware | UNTESTED | Linux NO-GO stopped dispatch |
| Atomic cleanup | hard | runtime/hardware | UNTESTED | Linux NO-GO stopped dispatch |

## Observations

### Linux 10-minute run (`reports/linux-10m.json`)

- **Frames produced:** 18,000 (600 s × 30 fps)
- **Encoder frames written:** 18,000 (no drops consumed by encoder)
- **Offer outcomes:** 18,000 Queued, 0 ReplacedOldest, 0 Disconnected
- **Offer latency (µs):** p50 = 1300, p99 = 1557, max = 4302, min = 394
- **Saturation windows:** 0 of 120 five-second windows above 10% replacement/drop threshold
- **Self RSS:** 26.5–27.8 MiB across 599 samples (1 MiB peak-to-trough per report), slope 0.13 MiB/min
- **FFmpeg RSS:** 450–728 MiB, stabilizing after warm-up with slow 0.69 MiB/min growth
- **Probe:** H.264, 1920×1080, 30/1 fps, duration 600,000 ms, 0 audio streams

The producer path never blocks and the queue never saturates, but the
clone-and-offer cost is consistently above the 1 ms hard gate. The p50 of
1300 µs means even the median offer takes 1.3 ms. The workload renderer
generates a 1920×1080 RGBA frame (~8 MiB) every ~33 ms; the clone alone
exceeds the 1 µs budget that a 1 ms p99 would require. This is a fundamental
cost of copying a full-frame buffer on every offer, not a transient spike.

## Final Recommendation

- Go / no-go: **NO-GO**
- Failed gate: ProducerP99 — p99 clone+offer latency 1557 µs exceeds 1000 µs hard limit
- Supporting evidence: reports/linux-10m.json (Linux, AMD Ryzen 7 255, 10-minute run, 18,000 frames)
- Rejected alternatives: none yet
- Fallback triggers: ProducerP99 gate failure requires a platform-native encoder design
  that avoids full-frame buffer copies on the capture thread (e.g., shared-memory
  ring buffer, zero-copy DMA, or GPU-side encode)
- Remaining risks: macOS runtime behavior untested
- Product handoff: do not write the production implementation plan. Return to
  `superpowers:brainstorming` for a platform-native encoder fallback design.

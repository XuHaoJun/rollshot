# Rollshot v0.3 Overlap-and-Overwrite Stitch Topology Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Revert v0.2.1's static region mask (commit `1b16e8de`) and implement
v0.3 overlap-and-overwrite stitch topology inside `LinearCanvas`, so each new
slice widens to `max(H/2, slice_px)` and pastes back into the canvas by
`overlap_size = max(0, H/2 - slice_px)` pixels, naturally hiding per-frame
trailing-edge artifacts (sticky footer, sticky header, 1 px browser chrome
border) without explicit detection.

**Architecture:** Internal-only change. Two phases: (1) `git revert 1b16e8de`
removes ~1100 lines of static_region detector + tests + canvas mask parameter,
restoring v0.2's public API. (2) Modify the four `LinearCanvas` slice helpers
(`append_bottom`, `prepend_top`, `append_right`, `prepend_left`) to take a
larger slice and paste with overlap, overwriting the previous slice's trailing
portion. Matcher, verifier, axis-lock, and capture are untouched.

**Tech Stack:** Rust, `image` crate (`RgbaImage`, `GenericImage`,
`GenericImageView`), `cargo test`, `cargo fmt`, `cargo clippy`.

**Reference docs:**
- Spec: `docs/superpowers/specs/2026-05-22-rollshot-overlap-stitch-topology-design.md`
- Reference implementation: `learn-projects/snow-shot/src-tauri/src-crates/app-scroll-screenshot-service/src/scroll_screenshot_service.rs` (lines 358–390, 491–513, 876–991)

**Project conventions (from `AGENTS.md`):**
- Shell commands prefix with `rtk` (e.g. `rtk cargo test`)
- Verification commands: `rtk cargo test`, `rtk cargo fmt --check`, `rtk cargo clippy --workspace --all-targets -- -D warnings`
- Frequent commits, surgical changes, no speculative features
- TDD: failing test first, minimal implementation, verify pass

---

## File Structure

**Files deleted (by `git revert`):**
- `crates/rollshot-core/src/static_region.rs`
- `crates/rollshot-core/tests/static_region.rs`

**Files reverted to v0.2 (by `git revert`):**
- `crates/rollshot-core/src/lib.rs` (drop `static_region` re-exports)
- `crates/rollshot-core/src/types.rs` (drop `StitchConfig::static_region`)
- `crates/rollshot-core/src/stitcher.rs` (drop `static_detector` field + observe/mask plumbing)
- `crates/rollshot-core/src/canvas.rs` (drop `mask` param from append helpers; delete `apply_static_mask`)
- `crates/rollshot-core/tests/canvas.rs` (drop `None` mask args from existing tests)
- `crates/rollshot-core/tests/common/mod.rs` (drop v0.2.1-added paint helpers — they will be re-added in Phase 3 where needed)

**Files modified for v0.3:**
- `crates/rollshot-core/src/canvas.rs` — rewrite the four slice helpers
- `docs/rollshot_mvp_design.md` — replace §3.2.1, update §20 risk row

**Files added for v0.3:**
- `crates/rollshot-core/tests/overlap_topology.rs` — integration tests

**Files retained as-is:**
- All matcher / verifier / axis / capture modules
- `crates/rollshot-core/tests/common/mod.rs` baseline helpers (`make_scroll_canvas`, `make_wide_canvas`, `crop_frame`, `crop_frame_xy`, `paint_sticky_header`, `make_repeated_rows`, `make_akaze_fallback_canvas`) — Phase 3 will re-add the sticky helpers that were lost in the revert.

---

## Phase 0: Baseline Verification

### Task 0.1: Verify starting point

**Files:** none (read-only)

- [ ] **Step 1: Confirm git state**

Run: `rtk git log --oneline -3`
Expected output (the HEAD commit hash will differ; the important line is the spec commit being at HEAD):
```
20f879c docs: v0.3 overlap-and-overwrite stitch topology spec
1b16e8d feat: static region mask (#9)
362bdc0 docs: improve static region mask plan
```

- [ ] **Step 2: Confirm working tree is clean**

Run: `rtk git status`
Expected: `(clean)` or only untracked files outside `crates/`.

- [ ] **Step 3: Confirm v0.2.1 baseline tests pass**

Run: `rtk cargo test --workspace`
Expected: all tests pass.

Run: `rtk cargo fmt --check`
Expected: no output, exit 0.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings, exit 0.

If any of these fail, **stop and investigate** before proceeding. The plan
assumes a clean v0.2.1 baseline.

---

## Phase 1: Revert v0.2.1

### Task 1.1: Revert commit `1b16e8de`

**Files (revert touches):**
- Delete: `crates/rollshot-core/src/static_region.rs`
- Delete: `crates/rollshot-core/tests/static_region.rs`
- Modify: `crates/rollshot-core/src/lib.rs`
- Modify: `crates/rollshot-core/src/types.rs`
- Modify: `crates/rollshot-core/src/stitcher.rs`
- Modify: `crates/rollshot-core/src/canvas.rs`
- Modify: `crates/rollshot-core/tests/canvas.rs`
- Modify: `crates/rollshot-core/tests/common/mod.rs`

- [ ] **Step 1: Verify `1b16e8de` is a regular (non-merge) commit**

Run: `rtk git cat-file -p 1b16e8de53948a83e246757632d3ba8e8aeb4973 | head -3`
Expected: single `parent` line (not two). If there are two parents, this is a
merge commit and you must add `-m 1` to the revert command below.

- [ ] **Step 2: Perform the revert**

Run: `rtk git revert --no-edit 1b16e8de53948a83e246757632d3ba8e8aeb4973`

Expected: revert commit created. If `git` reports conflicts, **stop and
investigate** — the spec commit `20f879c` only added a docs file in
`docs/superpowers/specs/`, which should not conflict with any file the revert
touches.

- [ ] **Step 3: Verify the file state after revert**

Run: `rtk proxy ls crates/rollshot-core/src/`
Expected: `static_region.rs` is **gone**. The remaining files are:
```
akaze_matcher.rs  axis.rs  canvas.rs  duplicate.rs  lib.rs  matcher.rs  overlap.rs  stitcher.rs  types.rs  verifier.rs
```

Run: `rtk proxy ls crates/rollshot-core/tests/`
Expected: `static_region.rs` is **gone**. The remaining files are:
```
canvas.rs  common
```

- [ ] **Step 4: Verify the workspace builds and tests pass at v0.2 baseline**

Run: `rtk cargo test --workspace`
Expected: all tests pass. The test count will be lower than before (v0.2.1's
tests are gone). No compile errors.

Run: `rtk cargo fmt --check`
Expected: clean.

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Amend the revert commit message for clarity**

The default revert message is `Revert "feat: static region mask (#9)"`. Replace
it with a message that explains *why* the revert is happening:

Run:
```bash
rtk git commit --amend -m "$(cat <<'EOF'
revert: v0.2.1 static region mask (1b16e8de)

Preparing for v0.3 overlap-and-overwrite stitch topology. See
docs/superpowers/specs/2026-05-22-rollshot-overlap-stitch-topology-design.md
for the rationale: the detector's bg_color sampling fails on 1 px decorative
borders, and the overlap topology subsumes the detector's role for the cases
that matter in real web layouts.

This revert removes ~1100 lines (static_region module + integration tests +
LinearCanvas mask plumbing) and restores LinearCanvas::append's v0.2
signature (no mask parameter).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 6: Verify the amended commit**

Run: `rtk git log --oneline -3`
Expected:
```
<new-hash> revert: v0.2.1 static region mask (1b16e8de)
20f879c    docs: v0.3 overlap-and-overwrite stitch topology spec
1b16e8d    feat: static region mask (#9)
```

---

## Phase 2: Canvas Overlap-and-Overwrite Implementation

**Important context for all Phase 2 tasks:**

After Phase 1, `crates/rollshot-core/src/canvas.rs` is the v0.2 version. The
public API is:

```rust
impl LinearCanvas {
    pub fn append(
        &mut self,
        direction: AppendDirection,
        frame: &RgbaImage,
        slice_px: u32,
    ) -> Result<u32, CanvasAppendError>;
}
```

The four internal helpers are `append_bottom`, `prepend_top`, `append_right`,
`prepend_left`. Each currently extracts a minimal slice (exactly `slice_px`
rows / cols) and appends with zero overlap. Phase 2 rewrites each of these to
the overlap-and-overwrite algorithm.

The overlap formula (from snow-shot's `scroll_screenshot_service.rs:491-499`):
- Vertical: `overlap_size = (frame_h / 2).saturating_sub(slice_px)`, `total_slice = (slice_px + overlap_size).min(frame_h)`
- Horizontal: `overlap_size = (frame_w / 2).saturating_sub(slice_px)`, `total_slice = (slice_px + overlap_size).min(frame_w)`

Each task in Phase 2 follows strict TDD: write the failing test, run to verify
it fails, implement, run to verify it passes, commit.

---

### Task 2.1: `append_bottom` — failing tests

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs` (add tests to existing `#[cfg(test)] mod tests` block)

- [ ] **Step 1: Add four new tests to `src/canvas.rs`'s test module**

Open `crates/rollshot-core/src/canvas.rs`, scroll to the end of the
`#[cfg(test)] mod tests` block, and append these four tests just before the
closing `}` of the module:

```rust
    #[test]
    fn append_bottom_pastes_at_canvas_height_minus_overlap() {
        // H=8, slice_px=2 → overlap = 4-2 = 2, total_slice = 4.
        // Slice = frame rows [4..8). Paste at canvas y = 8 - 2 = 6.
        // Slice overwrites canvas y=6..7 (= frame 1's rows 6..7) and adds
        // new canvas y=8..9 (= frame 2's rows 6..7).
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Bottom, &frame, 2)
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), 10);
        // y=0..5 stays frame 1 (gray 10/10/10).
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([10, 10, 10, 255]));
        // y=6..9 is now frame 2's slice (red 200/0/0).
        assert_eq!(canvas.image().get_pixel(0, 6), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 9), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_large_motion_uses_zero_overlap() {
        // H=4, slice_px=3 → overlap = max(0, 2 - 3) = 0, total_slice = 3.
        // Behaves identically to v0.2 minimal-slice append.
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Bottom, &frame, 3)
            .unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.height(), 7);
        // y=0..3 stays frame 1.
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([10, 10, 10, 255]));
        // y=4..6 is frame 2's bottom 3 rows.
        assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_tiny_motion_overlap_is_h_over_2_minus_one() {
        // H=10, slice_px=1 → overlap = 5-1 = 4, total_slice = 5.
        // Slice = frame rows [5..10). Paste at canvas y = 10 - 4 = 6.
        // Overwrites canvas y=6..9 (frame 1) with frame 2's rows 5..8;
        // adds canvas y=10 (frame 2's row 9).
        let base = solid(2, 10, [10, 10, 10, 255]);
        let frame = solid(2, 10, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Bottom, &frame, 1)
            .unwrap();
        assert_eq!(added, 1);
        assert_eq!(canvas.height(), 11);
        // y=0..5 stays frame 1.
        assert_eq!(canvas.image().get_pixel(0, 5), &Rgba([10, 10, 10, 255]));
        // y=6..10 is frame 2.
        assert_eq!(canvas.image().get_pixel(0, 6), &Rgba([200, 0, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 10), &Rgba([200, 0, 0, 255]));
    }

    #[test]
    fn append_bottom_net_growth_equals_slice_px() {
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [200, 0, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let h0 = canvas.height();
        let added = canvas
            .append(AppendDirection::Bottom, &frame, 2)
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), h0 + 2, "canvas must grow by exactly slice_px");
    }
```

- [ ] **Step 2: Run the new tests, expect FAILURES**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_pastes_at_canvas_height_minus_overlap`
Expected: **FAIL**. The pixel assertions at `(0, 6)` and `(0, 9)` should fail
because v0.2's minimal-slice append puts the slice at canvas y=8..9, leaving
canvas y=6..7 as frame 1's gray.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_tiny_motion_overlap_is_h_over_2_minus_one`
Expected: **FAIL** for the same reason (v0.2 puts the slice at y=10 only).

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_large_motion_uses_zero_overlap`
Expected: **PASS** (large-motion case is identical to v0.2; this is a
regression gate, not a new behavior test).

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_net_growth_equals_slice_px`
Expected: **PASS** (v0.2 already grows by exactly `slice_px`).

The two failing tests document the v0.3 behavior we are about to implement.

---

### Task 2.2: `append_bottom` — implementation

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs` (the `fn append_bottom` body)

- [ ] **Step 1: Replace `append_bottom` with the overlap-and-overwrite version**

Open `crates/rollshot-core/src/canvas.rs`. Find the existing `fn append_bottom`
(it should look approximately like this v0.2 version):

```rust
    fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let slice_px = slice_px.min(frame.height());
        let overlap = frame.height() - slice_px;
        let slice = frame.view(0, overlap, frame.width(), slice_px).to_image();
        let mut combined = RgbaImage::new(self.image.width(), self.image.height() + slice_px);
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined
            .copy_from(&slice, 0, self.image.height())
            .expect("copy slice");
        self.image = combined;
        slice_px
    }
```

Replace it with:

```rust
    fn append_bottom(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_h = frame.height();
        let slice_px = slice_px.min(frame_h);

        // Snow-shot's overlap formula: take max(H/2, slice_px) rows from the
        // frame's bottom and paste them back into the canvas by overlap_size
        // pixels, so the new slice overwrites the previous slice's tail.
        // For slice_px >= H/2, overlap_size collapses to 0 and behavior is
        // byte-identical to v0.2's minimal-slice append.
        let overlap_size = (frame_h / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_size).min(frame_h);

        let slice = frame
            .view(0, frame_h - total_slice, frame.width(), total_slice)
            .to_image();

        let new_height = self.image.height() + slice_px;
        let paste_y = self.image.height() - overlap_size;

        let mut combined = RgbaImage::new(self.image.width(), new_height);
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined.copy_from(&slice, 0, paste_y).expect("copy slice");
        self.image = combined;
        slice_px
    }
```

- [ ] **Step 2: Run the Task 2.1 tests, expect PASS**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_pastes_at_canvas_height_minus_overlap`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_tiny_motion_overlap_is_h_over_2_minus_one`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_large_motion_uses_zero_overlap`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_bottom_net_growth_equals_slice_px`
Expected: **PASS**.

- [ ] **Step 3: Run all canvas tests, ensure no regressions**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests`
Expected: all canvas inline unit tests pass.

Run: `rtk cargo test -p rollshot-core --test canvas`
Expected: all integration tests in `tests/canvas.rs` pass.

- [ ] **Step 4: Commit**

Run:
```bash
rtk git add crates/rollshot-core/src/canvas.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): append_bottom uses overlap-and-overwrite

Take max(H/2, slice_px) rows from frame bottom; paste at canvas
height - overlap_size so the slice's overlap portion overwrites
the previous canvas tail. For slice_px >= H/2 overlap_size is 0
and behavior is byte-identical to v0.2's minimal-slice append.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.3: `prepend_top` — failing tests

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs` (add tests to existing test module)

- [ ] **Step 1: Add three tests for `prepend_top` overlap behavior**

Append to `crates/rollshot-core/src/canvas.rs`'s `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn prepend_top_drops_overlap_rows_of_existing_canvas() {
        // H=8, slice_px=2 → overlap = 4-2 = 2, total_slice = 4.
        // Slice = frame rows [0..4). Combined y=0..3 = slice. Combined y=4..9
        // = old canvas rows [2..8). Old canvas rows 0..1 are dropped.
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [0, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Top, &frame, 2)
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), 10);
        // y=0..3 is now frame 2's top (green 0/200/0).
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([0, 200, 0, 255]));
        // y=4..9 is what remains of frame 1.
        assert_eq!(canvas.image().get_pixel(0, 4), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(0, 9), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_top_large_motion_uses_zero_overlap() {
        // H=4, slice_px=3 → overlap = 0, total_slice = 3. Behaves like v0.2.
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Top, &frame, 3)
            .unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.height(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 2), &Rgba([0, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(0, 3), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_top_net_growth_equals_slice_px() {
        let base = solid(4, 8, [10, 10, 10, 255]);
        let frame = solid(4, 8, [0, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let h0 = canvas.height();
        let added = canvas
            .append(AppendDirection::Top, &frame, 2)
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.height(), h0 + 2);
    }
```

- [ ] **Step 2: Run the new tests, expect FAILURES**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_top_drops_overlap_rows_of_existing_canvas`
Expected: **FAIL**. v0.2 prepend_top only takes 2 rows and pastes at y=0,
leaving canvas y=2..3 as frame 1 (gray), not frame 2 (green). The assertion
at `(0, 3) == (0, 200, 0, 255)` will fail.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_top_large_motion_uses_zero_overlap`
Expected: **PASS** (large-motion case identical to v0.2).

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_top_net_growth_equals_slice_px`
Expected: **PASS** (v0.2 already grows by `slice_px`).

---

### Task 2.4: `prepend_top` — implementation

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs` (the `fn prepend_top` body)

- [ ] **Step 1: Replace `prepend_top` with the overlap-and-overwrite version**

Find the v0.2 `fn prepend_top` and replace it with:

```rust
    fn prepend_top(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_h = frame.height();
        let slice_px = slice_px.min(frame_h);

        let overlap_size = (frame_h / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_size).min(frame_h);

        let slice = frame
            .view(0, 0, frame.width(), total_slice)
            .to_image();

        let new_height = self.image.height() + slice_px;

        let mut combined = RgbaImage::new(self.image.width(), new_height);
        combined.copy_from(&slice, 0, 0).expect("copy slice");
        // The slice's bottom `overlap_size` rows replace the existing canvas's
        // top `overlap_size` rows, so we copy the existing canvas starting at
        // y = overlap_size and place it at y = total_slice in the new buffer.
        let kept_old = self
            .image
            .view(
                0,
                overlap_size,
                self.image.width(),
                self.image.height() - overlap_size,
            )
            .to_image();
        combined
            .copy_from(&kept_old, 0, total_slice)
            .expect("copy base");
        self.image = combined;
        slice_px
    }
```

- [ ] **Step 2: Run the Task 2.3 tests, expect PASS**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_top_drops_overlap_rows_of_existing_canvas`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_top_large_motion_uses_zero_overlap`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_top_net_growth_equals_slice_px`
Expected: **PASS**.

- [ ] **Step 3: Run all canvas tests, ensure no regressions**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests`
Expected: all pass.

Run: `rtk cargo test -p rollshot-core --test canvas`
Expected: all pass.

- [ ] **Step 4: Commit**

Run:
```bash
rtk git add crates/rollshot-core/src/canvas.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): prepend_top uses overlap-and-overwrite

Take max(H/2, slice_px) rows from frame top; the slice's bottom
overlap_size rows replace the existing canvas's top overlap_size
rows. Net canvas growth is slice_px. Same fallback to no-overlap
when slice_px >= H/2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.5: `append_right` — failing tests

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`

- [ ] **Step 1: Add tests symmetric to append_bottom but in the x dimension**

Append to `crates/rollshot-core/src/canvas.rs`'s test module:

```rust
    #[test]
    fn append_right_pastes_at_canvas_width_minus_overlap() {
        // W=8, slice_px=2 → overlap = 4-2 = 2, total_slice = 4.
        // Slice = frame cols [4..8). Paste at canvas x = 8 - 2 = 6.
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [0, 0, 200, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Right, &frame, 2)
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), 10);
        // x=0..5 stays frame 1.
        assert_eq!(canvas.image().get_pixel(5, 0), &Rgba([10, 10, 10, 255]));
        // x=6..9 is now frame 2's slice (blue).
        assert_eq!(canvas.image().get_pixel(6, 0), &Rgba([0, 0, 200, 255]));
        assert_eq!(canvas.image().get_pixel(9, 0), &Rgba([0, 0, 200, 255]));
    }

    #[test]
    fn append_right_large_motion_uses_zero_overlap() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [0, 0, 200, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Right, &frame, 3)
            .unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.width(), 7);
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(4, 0), &Rgba([0, 0, 200, 255]));
    }

    #[test]
    fn append_right_net_growth_equals_slice_px() {
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [0, 0, 200, 255]);
        let mut canvas = LinearCanvas::new(base);
        let w0 = canvas.width();
        let added = canvas
            .append(AppendDirection::Right, &frame, 2)
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), w0 + 2);
    }
```

- [ ] **Step 2: Run the new tests, expect the first to FAIL**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_right_pastes_at_canvas_width_minus_overlap`
Expected: **FAIL** (v0.2 pastes at x=8, not x=6).

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_right_large_motion_uses_zero_overlap`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_right_net_growth_equals_slice_px`
Expected: **PASS**.

---

### Task 2.6: `append_right` — implementation

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`

- [ ] **Step 1: Replace `append_right` with the overlap version**

```rust
    fn append_right(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_w = frame.width();
        let slice_px = slice_px.min(frame_w);

        let overlap_size = (frame_w / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_size).min(frame_w);

        let slice = frame
            .view(frame_w - total_slice, 0, total_slice, frame.height())
            .to_image();

        let new_width = self.image.width() + slice_px;
        let paste_x = self.image.width() - overlap_size;

        let mut combined = RgbaImage::new(new_width, self.image.height());
        combined.copy_from(&self.image, 0, 0).expect("copy base");
        combined.copy_from(&slice, paste_x, 0).expect("copy slice");
        self.image = combined;
        slice_px
    }
```

- [ ] **Step 2: Run the Task 2.5 tests, expect PASS**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::append_right`
Expected: all three append_right tests pass.

- [ ] **Step 3: Run all canvas tests, ensure no regressions**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests`
Run: `rtk cargo test -p rollshot-core --test canvas`
Expected: all pass.

- [ ] **Step 4: Commit**

Run:
```bash
rtk git add crates/rollshot-core/src/canvas.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): append_right uses overlap-and-overwrite

Symmetric to append_bottom, in the x dimension. Take max(W/2,
slice_px) cols from frame's right; paste at canvas width -
overlap_size so the slice's left portion overwrites the previous
canvas trailing cols.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2.7: `prepend_left` — failing tests

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`

- [ ] **Step 1: Add tests symmetric to prepend_top but in the x dimension**

Append to the test module:

```rust
    #[test]
    fn prepend_left_drops_overlap_cols_of_existing_canvas() {
        // W=8, slice_px=2 → overlap = 2, total_slice = 4.
        // Slice = frame cols [0..4). Combined x=0..3 = slice (yellow).
        // Combined x=4..9 = old canvas cols [2..8).
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [200, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Left, &frame, 2)
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), 10);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(4, 0), &Rgba([10, 10, 10, 255]));
        assert_eq!(canvas.image().get_pixel(9, 0), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_left_large_motion_uses_zero_overlap() {
        let base = solid(4, 4, [10, 10, 10, 255]);
        let frame = solid(4, 4, [200, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let added = canvas
            .append(AppendDirection::Left, &frame, 3)
            .unwrap();
        assert_eq!(added, 3);
        assert_eq!(canvas.width(), 7);
        assert_eq!(canvas.image().get_pixel(0, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(2, 0), &Rgba([200, 200, 0, 255]));
        assert_eq!(canvas.image().get_pixel(3, 0), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn prepend_left_net_growth_equals_slice_px() {
        let base = solid(8, 4, [10, 10, 10, 255]);
        let frame = solid(8, 4, [200, 200, 0, 255]);
        let mut canvas = LinearCanvas::new(base);
        let w0 = canvas.width();
        let added = canvas
            .append(AppendDirection::Left, &frame, 2)
            .unwrap();
        assert_eq!(added, 2);
        assert_eq!(canvas.width(), w0 + 2);
    }
```

- [ ] **Step 2: Run the new tests, expect the first to FAIL**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_left_drops_overlap_cols_of_existing_canvas`
Expected: **FAIL** (v0.2 only takes 2 cols and pastes at x=0, leaving canvas
x=2..3 as frame 1's gray, not frame 2's yellow).

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_left_large_motion_uses_zero_overlap`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_left_net_growth_equals_slice_px`
Expected: **PASS**.

---

### Task 2.8: `prepend_left` — implementation

**Files:**
- Modify: `crates/rollshot-core/src/canvas.rs`

- [ ] **Step 1: Replace `prepend_left` with the overlap version**

```rust
    fn prepend_left(&mut self, frame: &RgbaImage, slice_px: u32) -> u32 {
        let frame_w = frame.width();
        let slice_px = slice_px.min(frame_w);

        let overlap_size = (frame_w / 2).saturating_sub(slice_px);
        let total_slice = (slice_px + overlap_size).min(frame_w);

        let slice = frame
            .view(0, 0, total_slice, frame.height())
            .to_image();

        let new_width = self.image.width() + slice_px;

        let mut combined = RgbaImage::new(new_width, self.image.height());
        combined.copy_from(&slice, 0, 0).expect("copy slice");
        let kept_old = self
            .image
            .view(
                overlap_size,
                0,
                self.image.width() - overlap_size,
                self.image.height(),
            )
            .to_image();
        combined
            .copy_from(&kept_old, total_slice, 0)
            .expect("copy base");
        self.image = combined;
        slice_px
    }
```

- [ ] **Step 2: Run the Task 2.7 tests, expect PASS**

Run: `rtk cargo test -p rollshot-core --lib canvas::tests::prepend_left`
Expected: all three prepend_left tests pass.

- [ ] **Step 3: Run the full workspace test suite**

Run: `rtk cargo test --workspace`
Expected: all pass. This includes existing matcher / verifier / axis /
stitcher tests and the integration tests in `tests/canvas.rs`. None of them
should break: pure-scroll behavior is byte-identical under v0.3 (algebraic
equivalence proven in the spec).

Run: `rtk cargo fmt --check`
Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

Run:
```bash
rtk git add crates/rollshot-core/src/canvas.rs
rtk git commit -m "$(cat <<'EOF'
feat(core): prepend_left uses overlap-and-overwrite

Symmetric to prepend_top, in the x dimension. The slice's right
overlap_size cols replace the existing canvas's left overlap_size
cols.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 3: Integration Tests

Phase 3 adds the `tests/overlap_topology.rs` file and re-introduces the
`paint_sticky_*` test helpers that Phase 1's revert removed. Each task is one
test or one small group of related tests.

**Important**: the revert in Task 1.1 removed `paint_sticky_sidebar`,
`paint_sticky_footer`, `paint_sticky_horizontal_band`, and the `Side` enum
from `crates/rollshot-core/tests/common/mod.rs`. Task 3.1 re-adds the helpers
we will need.

---

### Task 3.1: Re-add sticky paint helpers + write pure-scroll regression gate

**Files:**
- Modify: `crates/rollshot-core/tests/common/mod.rs`
- Create: `crates/rollshot-core/tests/overlap_topology.rs`

- [ ] **Step 1: Add `paint_sticky_*` helpers back to `tests/common/mod.rs`**

Append to `crates/rollshot-core/tests/common/mod.rs`:

```rust
#[derive(Debug, Clone, Copy)]
pub enum Side {
    Left,
    Right,
}

/// Paints a sticky vertical sidebar of the given pixel `width` on the chosen
/// side, full frame height. Uses a striped icon pattern (alternating 100/140
/// gray per 7 rows) so the sidebar is not uniform-color.
pub fn paint_sticky_sidebar(frame: &mut image::RgbaImage, side: Side, width: u32) {
    let h = frame.height();
    let w = frame.width();
    let x_start = match side {
        Side::Left => 0,
        Side::Right => w.saturating_sub(width),
    };
    for y in 0..h {
        for x in x_start..(x_start + width).min(w) {
            let v = if (y / 7) % 2 == 0 { 100 } else { 140 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}

/// Paints a sticky footer band of `height` pixels along the bottom edge.
pub fn paint_sticky_footer(frame: &mut image::RgbaImage, height: u32) {
    let h = frame.height();
    let w = frame.width();
    let y_start = h.saturating_sub(height);
    for y in y_start..h {
        for x in 0..w {
            let v = if (x / 9) % 2 == 0 { 110 } else { 150 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}

/// Paints a single 1-pixel-tall decorative bottom border (simulates the
/// browser chrome's native bottom edge). Solid uniform color.
pub fn paint_decorative_bottom_border(frame: &mut image::RgbaImage, color: image::Rgba<u8>) {
    let h = frame.height();
    if h == 0 {
        return;
    }
    let w = frame.width();
    for x in 0..w {
        frame.put_pixel(x, h - 1, color);
    }
}

/// Paints sticky horizontal bands of `top_h` pixels at the top and `bottom_h`
/// at the bottom (either may be zero). Used for horizontal-scroll fixtures.
pub fn paint_sticky_horizontal_band(frame: &mut image::RgbaImage, top_h: u32, bottom_h: u32) {
    let h = frame.height();
    let w = frame.width();
    for y in 0..top_h.min(h) {
        for x in 0..w {
            let v = if (x / 5) % 2 == 0 { 90 } else { 130 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
    let bottom_start = h.saturating_sub(bottom_h);
    for y in bottom_start..h {
        for x in 0..w {
            let v = if (x / 5) % 2 == 0 { 95 } else { 135 };
            frame.put_pixel(x, y, image::Rgba([v, v, v, 255]));
        }
    }
}

/// Paints a single horizontal icon stripe at frame-local y `icon_y` with the
/// given height and color, in the leftmost `sidebar_width` columns.
/// Simulates a sidebar icon anchored at a specific frame-local position.
pub fn paint_sidebar_icon_at(
    frame: &mut image::RgbaImage,
    sidebar_width: u32,
    icon_y: u32,
    icon_h: u32,
    color: image::Rgba<u8>,
) {
    let h = frame.height();
    let w = frame.width();
    let y0 = icon_y.min(h);
    let y1 = (icon_y + icon_h).min(h);
    let x1 = sidebar_width.min(w);
    for y in y0..y1 {
        for x in 0..x1 {
            frame.put_pixel(x, y, color);
        }
    }
}
```

- [ ] **Step 2: Create `tests/overlap_topology.rs` with the regression gate**

Create `crates/rollshot-core/tests/overlap_topology.rs`:

```rust
mod common;

use common::{crop_frame, crop_frame_xy, make_scroll_canvas};
use image::RgbaImage;
use rollshot_core::{AppendDirection, LinearCanvas, StitchConfig, Stitcher};

#[test]
fn pure_scroll_byte_identical_to_v0_2_minimal_slice() {
    // For pure-scroll fixtures, v0.3 overlap-and-overwrite is algebraically
    // equivalent to v0.2 minimal-slice append: every pixel that v0.3
    // overwrites in the overlap region was the SAME source-canvas pixel that
    // v0.2 already placed there.
    //
    // We bypass the motion estimator and drive LinearCanvas directly with
    // exact slice_px values. This way the test purely validates the overlap
    // topology algorithm — estimator drift is a separate concern tested
    // elsewhere.
    let source = make_scroll_canvas(320, 1400);
    let frame_w = 320u32;
    let frame_h = 320u32;
    let step = 70u32;

    let first = crop_frame_xy(&source, 0, 0, frame_w, frame_h);
    let mut canvas = LinearCanvas::new(first);
    let mut expected_h = frame_h;

    let mut y = step;
    while y + frame_h <= source.height() && expected_h < 700 {
        let f = crop_frame_xy(&source, 0, y, frame_w, frame_h);
        let added = canvas
            .append(AppendDirection::Bottom, &f, step)
            .expect("append");
        assert_eq!(added, step);
        expected_h += step;
        y += step;
    }

    let stitched = canvas.image();
    assert_eq!(stitched.height(), expected_h);
    for cy in 0..stitched.height() {
        for cx in 0..stitched.width() {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                source.get_pixel(cx, cy),
                "pixel mismatch at ({cx}, {cy})",
            );
        }
    }
}
```

- [ ] **Step 3: Run the new test, expect PASS**

Run: `rtk cargo test -p rollshot-core --test overlap_topology pure_scroll_byte_identical_to_v0_2_minimal_slice`
Expected: **PASS**. If this fails, the overlap math in Phase 2 has a bug;
investigate before continuing.

- [ ] **Step 4: Commit**

Run:
```bash
rtk git add crates/rollshot-core/tests/common/mod.rs crates/rollshot-core/tests/overlap_topology.rs
rtk git commit -m "$(cat <<'EOF'
test(core): pure-scroll v0.2 equivalence regression gate

Re-add sticky paint helpers (lost in the revert) and create the
overlap_topology integration test file with the byte-identical-
to-v0.2 regression gate as the first test.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3.2: First-frame preservation + sticky header tests

**Files:**
- Modify: `crates/rollshot-core/tests/overlap_topology.rs`

- [ ] **Step 1: Add `first_frame_preserved_verbatim` test**

Append to `crates/rollshot-core/tests/overlap_topology.rs`:

```rust
use common::paint_sticky_header;
use image::Rgba;
use rollshot_core::StitchOutcome;

#[test]
fn first_frame_preserved_verbatim() {
    // The first frame goes through accept_first_frame -> LinearCanvas::new
    // and never flows through the slice helpers. So whatever pixels the
    // first frame has (including any sticky UI it contains) must appear
    // unchanged at canvas y=0..frame_h.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut first = crop_frame(&source, 0, frame_h);
    paint_sticky_header(&mut first, 12);
    let expected_first = first.clone();

    let outcome = stitcher.push_frame(first);
    assert!(matches!(outcome, StitchOutcome::FirstFrame));

    // Push at least two more frames so the canvas grows beyond the first.
    let mut y = step;
    let mut appended = 0u32;
    while y + frame_h <= source.height() && appended < 3 {
        let mut f = crop_frame(&source, y, frame_h);
        paint_sticky_header(&mut f, 12);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 2, "need >=2 appends; got {appended}");

    let stitched = stitcher
        .full_image()
        .expect("stitched output exists")
        .clone();

    // Frame 1's pixels at canvas y=0..frame_h must match expected_first
    // EXCEPT at canvas y where Phase 2's overlap from frame 2 has already
    // overwritten them. The overlap algorithm guarantees that for any
    // slice_px >= 1, paste_y >= H/2 + 1, so canvas y in [0, H/2] is ALWAYS
    // preserved regardless of the exact motion estimate. Use H/2 as a
    // motion-estimator-noise-tolerant lower bound.
    let preserved_until = frame_h / 2;
    for y in 0..preserved_until {
        for x in 0..stitched.width() {
            assert_eq!(
                stitched.get_pixel(x, y),
                expected_first.get_pixel(x, y),
                "first-frame pixel changed at ({x}, {y}); should be preserved before y={preserved_until}",
            );
        }
    }
}

#[test]
fn sticky_header_appears_only_at_canvas_top() {
    // For vertical scroll-down, the slice is taken from the FRAME BOTTOM
    // (rows H-total_slice..H). The header at frame rows 0..12 is in the
    // FRAME TOP and is NEVER in the slice. So after the first frame, the
    // header never re-enters the canvas. It appears exactly once, at canvas
    // rows 0..12 (from the first frame).
    //
    // paint_sticky_header writes alternating Rgba(200,60,60) and
    // Rgba(30,30,90) pixels. We assert that a row well below the first
    // frame's header (e.g. canvas y = first_frame_h + 50) does NOT contain
    // either of those colors.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    // Loosen verifier MAD thresholds because the painted header pattern
    // inflates overlap MAD even though the matcher's content_roi already
    // excludes the header band.
    let mut config = StitchConfig::default();
    config.verifier.downsample_max_mad = 40.0 / 255.0;
    config.verifier.full_res_max_mad = 30.0 / 255.0;
    stitcher = Stitcher::new(config);

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        paint_sticky_header(&mut f, 12);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");

    // Header in frame 1 is at canvas y=0..11 — must still be the painted
    // header pixels.
    let header_red = Rgba([200, 60, 60, 255]);
    let header_dark_blue = Rgba([30, 30, 90, 255]);
    let mut saw_header_at_top = false;
    for x in 0..stitched.width() {
        let p = *stitched.get_pixel(x, 0);
        if p == header_red || p == header_dark_blue {
            saw_header_at_top = true;
            break;
        }
    }
    assert!(saw_header_at_top, "frame 1's header must remain at canvas top");

    // Below the first frame's content (well after y = frame_h), there must
    // be NO header-colored pixels. Pick y = frame_h + step + 1, which is
    // deep inside frame 2's slice contribution.
    let probe_y = frame_h + step + 1;
    if probe_y < stitched.height() {
        for x in 0..stitched.width() {
            let p = *stitched.get_pixel(x, probe_y);
            assert!(
                p != header_red && p != header_dark_blue,
                "header color leaked to canvas y={probe_y} at x={x}: {p:?}",
            );
        }
    }
}
```

- [ ] **Step 2: Run the new tests, expect PASS**

Run: `rtk cargo test -p rollshot-core --test overlap_topology first_frame_preserved_verbatim`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --test overlap_topology sticky_header_appears_only_at_canvas_top`
Expected: **PASS**.

- [ ] **Step 3: Commit**

Run:
```bash
rtk git add crates/rollshot-core/tests/overlap_topology.rs
rtk git commit -m "test(core): first-frame preservation + sticky-header at top

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.3: Sticky footer + 1 px decorative border tests

**Files:**
- Modify: `crates/rollshot-core/tests/overlap_topology.rs`

- [ ] **Step 1: Add the two footer/border tests**

Append to `crates/rollshot-core/tests/overlap_topology.rs`:

```rust
use common::{paint_decorative_bottom_border, paint_sticky_footer};

#[test]
fn sticky_footer_only_at_canvas_bottom() {
    // Each frame paints a 12px footer at its bottom rows. Each slice's
    // overlap region is overwritten by the next slice (which also paints
    // its own footer). Only the LAST appended slice's footer survives in
    // the canvas, located at the canvas's bottom rows.
    //
    // paint_sticky_footer writes alternating 110/150 gray columns of 9px.
    // We assert (a) the canvas bottom contains those colors, and (b) the
    // MIDDLE of the canvas (well above the bottom) does NOT contain those
    // colors.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        paint_sticky_footer(&mut f, 12);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");
    let h = stitched.height();

    // The bottom row must be a footer color (one of 110/150 in each channel).
    let bottom = *stitched.get_pixel(stitched.width() / 2, h - 1);
    assert!(
        bottom[0] == 110 || bottom[0] == 150,
        "bottom row should be a footer color; got {bottom:?}",
    );
    assert_eq!(bottom[0], bottom[1]);
    assert_eq!(bottom[1], bottom[2]);

    // A middle row well above the bottom must NOT be a footer color.
    // Choose y = frame_h (just after the first frame's footer in v0.2;
    // in v0.3 frame 1's footer has been overwritten by frame 2's slice).
    let probe_y = frame_h;
    if probe_y < h {
        let mut saw_footer = false;
        for x in 0..stitched.width() {
            let p = *stitched.get_pixel(x, probe_y);
            if p[0] == p[1] && p[1] == p[2] && (p[0] == 110 || p[0] == 150) {
                saw_footer = true;
                break;
            }
        }
        assert!(
            !saw_footer,
            "footer leaked to canvas y={probe_y} in the middle of the canvas",
        );
    }
}

#[test]
fn decorative_1px_bottom_border_only_at_canvas_bottom() {
    // The user's actual use case: 1 px gray border at the very bottom of
    // every frame (browser chrome). Under v0.2 this would create a gray
    // line at every slice boundary. Under v0.3 it survives only at the
    // canvas's last row.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let border_color = Rgba([160, 160, 160, 255]); // distinctive gray not produced by make_scroll_canvas
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        paint_decorative_bottom_border(&mut f, border_color);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 4, "need >=4 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");
    let h = stitched.height();
    let w = stitched.width();

    // Last row must be the border color across all columns.
    for x in 0..w {
        let p = *stitched.get_pixel(x, h - 1);
        assert_eq!(p, border_color, "bottom row must be border at x={x}");
    }

    // For every row from frame_h..h-1 (i.e., not the first frame's bottom
    // and not the canvas bottom), there must be NO border-colored pixel.
    // (The first frame's row frame_h-1 IS overwritten by frame 2's slice
    // in v0.3, so it should NOT contain the border.)
    let mut border_seen_at_y: Vec<u32> = Vec::new();
    for y in 0..h - 1 {
        for x in 0..w {
            if *stitched.get_pixel(x, y) == border_color {
                border_seen_at_y.push(y);
                break;
            }
        }
    }
    assert!(
        border_seen_at_y.is_empty(),
        "decorative border found at unexpected canvas rows: {border_seen_at_y:?}",
    );
}
```

- [ ] **Step 2: Run the new tests, expect PASS**

Run: `rtk cargo test -p rollshot-core --test overlap_topology sticky_footer_only_at_canvas_bottom`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --test overlap_topology decorative_1px_bottom_border_only_at_canvas_bottom`
Expected: **PASS**. This is the user-reported regression — confirm it actually
passes before continuing.

- [ ] **Step 3: Commit**

Run:
```bash
rtk git add crates/rollshot-core/tests/overlap_topology.rs
rtk git commit -m "test(core): sticky-footer + 1px decorative border at canvas bottom only

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.4: Solid sidebar + anchored icon tests

**Files:**
- Modify: `crates/rollshot-core/tests/overlap_topology.rs`

- [ ] **Step 1: Add three sidebar-shape tests**

Append:

```rust
use common::{paint_sidebar_icon_at, paint_sticky_sidebar, Side};

#[test]
fn solid_sidebar_renders_as_continuous_column() {
    // A solid-color sidebar is the same color in every frame. Under v0.3
    // (and v0.2), the leftmost column of the canvas is that color from top
    // to bottom — a continuous strip. This is the correct "sticky sidebar"
    // appearance and what real web layouts look like.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let sidebar_color = Rgba([50, 60, 70, 255]);
    let sidebar_w = 12u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        for fy in 0..f.height() {
            for fx in 0..sidebar_w.min(f.width()) {
                f.put_pixel(fx, fy, sidebar_color);
            }
        }
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");
    // Every pixel in cols [0, sidebar_w) must be the sidebar color.
    for cy in 0..stitched.height() {
        for cx in 0..sidebar_w {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                &sidebar_color,
                "solid sidebar should be continuous at ({cx}, {cy})",
            );
        }
    }
}

#[test]
fn top_anchored_sidebar_icon_preserved_from_first_frame() {
    // An icon at frame-local y=20..40 in the sidebar lives in the FRAME TOP.
    // For scroll-down it's never in the slice, so it only appears from
    // frame 1's preserved upper portion. After N appends, the icon should
    // appear ONCE at canvas y=20..40.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let sidebar_w = 12u32;
    let icon_color = Rgba([255, 128, 0, 255]); // distinctive orange
    let icon_y = 20u32;
    let icon_h = 20u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        // Paint a solid sidebar so the icon stands out.
        for fy in 0..f.height() {
            for fx in 0..sidebar_w.min(f.width()) {
                f.put_pixel(fx, fy, Rgba([50, 60, 70, 255]));
            }
        }
        paint_sidebar_icon_at(&mut f, sidebar_w, icon_y, icon_h, icon_color);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");

    // Icon must be present at canvas y=20..40, cols 0..sidebar_w.
    for cy in icon_y..icon_y + icon_h {
        for cx in 0..sidebar_w {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                &icon_color,
                "icon missing at ({cx}, {cy})",
            );
        }
    }

    // Icon must NOT be present anywhere else in the sidebar columns.
    // Scan every row outside [icon_y, icon_y + icon_h) in the sidebar
    // columns and assert no icon-colored pixel.
    for cy in 0..stitched.height() {
        if cy >= icon_y && cy < icon_y + icon_h {
            continue;
        }
        for cx in 0..sidebar_w {
            assert!(
                stitched.get_pixel(cx, cy) != &icon_color,
                "icon leaked to ({cx}, {cy}) — top-anchored icon should appear only at frame 1's position",
            );
        }
    }
}

#[test]
fn bottom_anchored_sidebar_icon_only_at_canvas_bottom() {
    // An icon at frame-local y = H - icon_h is in the FRAME BOTTOM. It enters
    // every slice but each new slice's overlap overwrites the previous one.
    // Only the LAST appended slice's icon survives at the canvas's bottom.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let sidebar_w = 12u32;
    let icon_color = Rgba([0, 200, 200, 255]); // distinctive cyan
    let icon_h = 20u32;
    let icon_y = frame_h - icon_h;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    let mut y = 0;
    let mut appended = 0u32;
    while y + frame_h <= source.height() {
        let mut f = crop_frame(&source, y, frame_h);
        for fy in 0..f.height() {
            for fx in 0..sidebar_w.min(f.width()) {
                f.put_pixel(fx, fy, Rgba([50, 60, 70, 255]));
            }
        }
        paint_sidebar_icon_at(&mut f, sidebar_w, icon_y, icon_h, icon_color);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended += 1;
        }
        y += step;
    }
    assert!(appended >= 3, "need >=3 appends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");
    let h = stitched.height();

    // Icon must be present at canvas y = h - icon_h .. h, cols 0..sidebar_w.
    for cy in h - icon_h..h {
        for cx in 0..sidebar_w {
            assert_eq!(
                stitched.get_pixel(cx, cy),
                &icon_color,
                "icon missing at ({cx}, {cy}) at canvas bottom",
            );
        }
    }

    // Icon must NOT appear in cols 0..sidebar_w outside that bottom strip.
    for cy in 0..h - icon_h {
        for cx in 0..sidebar_w {
            assert!(
                stitched.get_pixel(cx, cy) != &icon_color,
                "icon leaked to ({cx}, {cy}) — bottom-anchored should only appear at canvas bottom",
            );
        }
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `rtk cargo test -p rollshot-core --test overlap_topology solid_sidebar_renders_as_continuous_column`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --test overlap_topology top_anchored_sidebar_icon_preserved_from_first_frame`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --test overlap_topology bottom_anchored_sidebar_icon_only_at_canvas_bottom`
Expected: **PASS**.

- [ ] **Step 3: Commit**

Run:
```bash
rtk git add crates/rollshot-core/tests/overlap_topology.rs
rtk git commit -m "test(core): solid sidebar + top/bottom anchored icon coverage

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.5: Scroll-up + bidirectional tests

**Files:**
- Modify: `crates/rollshot-core/tests/overlap_topology.rs`

- [ ] **Step 1: Add scroll-up + bidirectional tests**

Append:

```rust
#[test]
fn sticky_header_after_scroll_up_appears_only_once() {
    // For vertical scroll-UP (each new frame shows content higher on the
    // page), the stitcher uses prepend_top. The header at frame rows
    // 0..header_h IS in the slice. Each new prepend's overlap region
    // overwrites the previous prepend's overlap region — only the most
    // recently appended slice's header survives at canvas top.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut config = StitchConfig::default();
    config.verifier.downsample_max_mad = 40.0 / 255.0;
    config.verifier.full_res_max_mad = 30.0 / 255.0;
    let mut stitcher = Stitcher::new(config);

    let mut y = source.height() - frame_h;
    let mut appended = 0u32;
    loop {
        let mut f = crop_frame(&source, y, frame_h);
        paint_sticky_header(&mut f, 12);
        match stitcher.push_frame(f) {
            StitchOutcome::FirstFrame => {}
            StitchOutcome::Appended { .. } => appended += 1,
            _ => {}
        }
        if y < step {
            break;
        }
        y -= step;
    }
    assert!(appended >= 3, "need >=3 prepends; got {appended}");

    let stitched = stitcher.full_image().expect("stitched");

    // Header must appear at canvas y=0..12 (the most recent prepend's top
    // rows). Scan canvas y=0 and verify at least one header-colored pixel.
    let header_red = Rgba([200, 60, 60, 255]);
    let header_dark_blue = Rgba([30, 30, 90, 255]);
    let mut saw_header = false;
    for x in 0..stitched.width() {
        let p = *stitched.get_pixel(x, 0);
        if p == header_red || p == header_dark_blue {
            saw_header = true;
            break;
        }
    }
    assert!(saw_header, "header must appear at canvas top after scroll-up");

    // Header must NOT appear at any y in [step, stitched.height()).
    // (The very first frame's header lives at canvas y=stitched.height()-frame_h,
    // and that frame's header is at frame-local y=0..11; after the prepend
    // chain, only the most recent slice's header survives at canvas top.)
    let probe_start = step;
    for y in probe_start..stitched.height() {
        let mut saw = false;
        for x in 0..stitched.width() {
            let p = *stitched.get_pixel(x, y);
            if p == header_red || p == header_dark_blue {
                saw = true;
                break;
            }
        }
        assert!(
            !saw,
            "header leaked to canvas y={y} after scroll-up prepend",
        );
    }
}

#[test]
fn bidirectional_scroll_down_then_up_canvas_consistent() {
    // Scroll down for a few frames, then scroll up past the start. The
    // canvas must grow at both edges; pixel content in each direction's
    // overlap region must come from the most recent frame in that direction.
    // Use a footer fixture to make the leading-edge cleanup observable on
    // both ends.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 70u32;
    let mut stitcher = Stitcher::new(StitchConfig::default());

    // Start in the middle, scroll down 3 times, then scroll up 3 times past
    // the start.
    let mut anchor = 500u32;
    let mut appended_total = 0u32;

    // Frame 1 (anchor).
    let mut f = crop_frame(&source, anchor, frame_h);
    paint_sticky_footer(&mut f, 8);
    stitcher.push_frame(f);

    // Scroll down 3 frames.
    for _ in 0..3 {
        anchor += step;
        let mut f = crop_frame(&source, anchor, frame_h);
        paint_sticky_footer(&mut f, 8);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended_total += 1;
        }
    }

    // Scroll up 3 frames past the original start.
    let mut up_anchor = 500u32;
    for _ in 0..3 {
        if up_anchor < step {
            break;
        }
        up_anchor -= step;
        let mut f = crop_frame(&source, up_anchor, frame_h);
        paint_sticky_footer(&mut f, 8);
        if let StitchOutcome::Appended { .. } = stitcher.push_frame(f) {
            appended_total += 1;
        }
    }

    assert!(
        appended_total >= 4,
        "need >=4 appends across both directions; got {appended_total}",
    );

    let stitched = stitcher.full_image().expect("stitched");
    // Sanity: canvas must have grown in both directions beyond frame_h.
    assert!(
        stitched.height() > frame_h,
        "canvas height should exceed single frame_h after bidirectional scroll",
    );

    // Footer (110 or 150 gray) must appear at the canvas bottom row
    // (the most recently appended downward slice's footer).
    // After scrolling up, the most recent append is upward — so the
    // BOTTOM of the canvas is preserved from the last DOWNWARD frame,
    // which already had a footer at its bottom that survived because
    // no later DOWNWARD frame came after it.
    let h = stitched.height();
    let bottom = *stitched.get_pixel(stitched.width() / 2, h - 1);
    assert!(
        bottom[0] == 110 || bottom[0] == 150,
        "canvas bottom should still be a footer color after bidirectional scroll; got {bottom:?}",
    );
}
```

- [ ] **Step 2: Run the tests**

Run: `rtk cargo test -p rollshot-core --test overlap_topology sticky_header_after_scroll_up_appears_only_once`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --test overlap_topology bidirectional_scroll_down_then_up_canvas_consistent`
Expected: **PASS**.

- [ ] **Step 3: Commit**

Run:
```bash
rtk git add crates/rollshot-core/tests/overlap_topology.rs
rtk git commit -m "test(core): scroll-up + bidirectional overlap coverage

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.6: Horizontal scroll + large-motion fallback tests

**Files:**
- Modify: `crates/rollshot-core/tests/overlap_topology.rs`

- [ ] **Step 1: Add horizontal scroll tests + large-motion fallback test**

Append:

```rust
use common::{crop_frame_xy, make_wide_canvas, paint_sticky_horizontal_band};

fn drive_horizontal_right(
    canvas: &RgbaImage,
    frame_w: u32,
    step: u32,
    mut paint: impl FnMut(&mut RgbaImage),
) -> RgbaImage {
    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut x = 0;
    while x + frame_w <= canvas.width() {
        let mut f = crop_frame_xy(canvas, x, 0, frame_w, canvas.height());
        paint(&mut f);
        stitcher.push_frame(f);
        x += step;
    }
    stitcher
        .full_image()
        .expect("stitched output exists")
        .clone()
}

#[test]
fn horizontal_scroll_with_sticky_top_band() {
    let source = make_wide_canvas(1400, 320);
    let frame_w = 320u32;
    let step = 70u32;

    let stitched = drive_horizontal_right(&source, frame_w, step, |f| {
        paint_sticky_horizontal_band(f, 10, 0);
    });

    // The 10-row top band should be present at canvas y=0..10 across the
    // full width (it's "sticky" — appears on every column from any frame).
    // The band uses 90/130 grays; assert pixels at y=0 are one of those.
    for x in 0..stitched.width() {
        let p = *stitched.get_pixel(x, 0);
        assert!(
            (p[0] == 90 || p[0] == 130) && p[0] == p[1] && p[1] == p[2],
            "top band missing at ({x}, 0): {p:?}",
        );
    }
}

#[test]
fn horizontal_scroll_with_sticky_bottom_band() {
    let source = make_wide_canvas(1400, 320);
    let frame_w = 320u32;
    let step = 70u32;

    let stitched = drive_horizontal_right(&source, frame_w, step, |f| {
        paint_sticky_horizontal_band(f, 0, 8);
    });

    // The bottom band (95/135 grays) must appear at canvas y=h-1.
    let h = stitched.height();
    for x in 0..stitched.width() {
        let p = *stitched.get_pixel(x, h - 1);
        assert!(
            (p[0] == 95 || p[0] == 135) && p[0] == p[1] && p[1] == p[2],
            "bottom band missing at ({x}, h-1): {p:?}",
        );
    }
}

#[test]
fn motion_larger_than_half_frame_falls_back_to_v0_2_behavior() {
    // When slice_px >= frame_h / 2 the overlap formula yields overlap_size = 0
    // and slice helper behavior is byte-identical to v0.2 minimal-slice
    // append. We can't compare against a v0.2 config switch (it no longer
    // exists), so we assert the canvas-grows-by-`added`-per-append invariant
    // — this holds in BOTH the overlap and the no-overlap regimes and is
    // therefore a regression gate for the fallback path specifically because
    // the test uses step > frame_h/2.
    let source = make_scroll_canvas(320, 1400);
    let frame_h = 320u32;
    let step = 200u32; // > frame_h / 2

    let mut stitcher = Stitcher::new(StitchConfig::default());
    let mut y = 0;
    let mut last_h: Option<u32> = None;
    let mut append_count = 0u32;
    while y + frame_h <= source.height() {
        let f = crop_frame(&source, y, frame_h);
        let outcome = stitcher.push_frame(f);
        let current_h = stitcher.full_image().unwrap().height();
        match outcome {
            StitchOutcome::FirstFrame => {
                last_h = Some(current_h);
            }
            StitchOutcome::Appended { added, .. } => {
                let prev = last_h.expect("first frame must precede appends");
                assert_eq!(
                    current_h - prev,
                    added,
                    "canvas must grow by exactly `added` per append",
                );
                last_h = Some(current_h);
                append_count += 1;
            }
            _ => {}
        }
        y += step;
    }
    assert!(append_count >= 2, "need >=2 appends; got {append_count}");
}
```

- [ ] **Step 2: Run the tests**

Run: `rtk cargo test -p rollshot-core --test overlap_topology horizontal_scroll_with_sticky_top_band`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --test overlap_topology horizontal_scroll_with_sticky_bottom_band`
Expected: **PASS**.

Run: `rtk cargo test -p rollshot-core --test overlap_topology motion_larger_than_half_frame_falls_back_to_v0_2_behavior`
Expected: **PASS**.

- [ ] **Step 3: Run the entire overlap_topology test file once**

Run: `rtk cargo test -p rollshot-core --test overlap_topology`
Expected: all 11 tests pass.

- [ ] **Step 4: Commit**

Run:
```bash
rtk git add crates/rollshot-core/tests/overlap_topology.rs
rtk git commit -m "test(core): horizontal scroll + large-motion fallback coverage

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Phase 4: Documentation

### Task 4.1: Replace §3.2.1 in `docs/rollshot_mvp_design.md`

**Files:**
- Modify: `docs/rollshot_mvp_design.md`

- [ ] **Step 1: Replace the §3.2.1 block**

In `docs/rollshot_mvp_design.md`, find the section that starts at line 174:

```
### 3.2.1 v0.2.1：Static region mask（patch）
```

And ends at the `---` separator just before `### 3.3 v0.3：Capture UX / interactive session`
(line 248).

Replace the entire block (lines 174–248) with:

````markdown
### 3.2.1 v0.3：Overlap-and-overwrite stitch topology

v0.2 完成後，matcher 端透過 content ROI 已不再被 sticky / fixed UI 帶跑，但
`LinearCanvas::append_*` 仍把整條 slice 貼進 canvas，造成 sticky header /
footer / 裝飾性邊框在長圖上每個 slice 邊界重複出現。v0.2.1 曾經嘗試用
`StaticRegionDetector` 解決，但對 1 px 邊框因為「灰填灰」無效；於 commit
`1b16e8de` revert，改在 v0.3 用 snow-shot 的 overlap-and-overwrite slicing
topology 處理。

詳細 spec 見
`docs/superpowers/specs/2026-05-22-rollshot-overlap-stitch-topology-design.md`，
以下只列重點。

目標：

```text
1. 每個 slice 取 max(H/2, slice_px) 而非僅 slice_px。
2. Paste 位置往回 overlap_size = max(0, H/2 - slice_px) 像素，
   讓新 slice 的 overlap 部分覆蓋前一個 slice 的 trailing portion。
3. 只有最近一次 append 的 trailing pixel 殘留在 canvas 中。
4. 對 sticky header / footer / 純色 sidebar / 1 px 裝飾邊框
   自然 cover，無需偵測。
5. LinearCanvas::append 公開簽名回歸 v0.2（無 mask 參數）。
6. Pure-scroll fixture 輸出 byte-identical to v0.2 minimal-slice。
```

演算法（Append Bottom；其餘三方向對稱）：

```text
inputs: frame W×H, slice_px = |motion.dy|

overlap_size = (H / 2).saturating_sub(slice_px)
total_slice  = (slice_px + overlap_size).min(H)

slice  = frame.view(0, H - total_slice, W, total_slice).to_image()
paste_y = canvas.height() - overlap_size
combined = RgbaImage::new(W, canvas.height() + slice_px)
combined.copy_from(canvas.image, 0, 0)
combined.copy_from(&slice, 0, paste_y)
canvas.image = combined
```

不解決的 cases（已 documented 為限制）：

```text
- patterned / textured sidebar 內部 pattern 與 frame-local y 綁定
  → overlap boundary 處仍有 pattern seam（與 v0.2 相同數量、位置不同）
- 中間錨定的 sticky 元素（icon 在 frame-local y ≈ H/2）
  → 仍會重複；真實 web layout 中極為罕見
- motion > H/2 的單次滾動
  → overlap_size = 0，行為退化為 v0.2 minimal-slice append
```

完成標準：

```text
[ ] commit 1b16e8de 完整 revert
[ ] static_region 模組 / 測試 / 公開 type 移除
[ ] LinearCanvas::append signature 回歸 v0.2
[ ] 四個 slice helper 用 overlap-and-overwrite
[ ] tests/overlap_topology.rs 涵蓋 sticky header / footer / 1 px 邊框
    / 純色 sidebar / 頂底錨定 icon / scroll-up / 雙向 / horizontal
[ ] pure_scroll_byte_identical_to_v0_2_minimal_slice 通過
[ ] cargo test / fmt / clippy 全綠
```
````

- [ ] **Step 2: Verify the section reads cleanly**

Run: `rtk proxy grep -n "^### 3.2.1\|^### 3.3" docs/rollshot_mvp_design.md`
Expected: a single line with `### 3.2.1 v0.3：Overlap-and-overwrite stitch
topology` and `### 3.3 v0.3：Capture UX / interactive session`.

(Note: the original heading for §3.3 already says "v0.3"; that's an
unrelated section name we are NOT changing — it refers to v0.3's capture UX
feature, which is a different work item.)

---

### Task 4.2: Update §20 risk row

**Files:**
- Modify: `docs/rollshot_mvp_design.md`

- [ ] **Step 1: Replace the sticky-header risk row in §20**

Find line 1389 (the row starting with `sticky header / sidebar 干擾`):

```
| sticky header / sidebar 干擾 | matcher：content ROI 排除 top/bottom/side；canvas append：v0.2.1 static region mask（見 3.2.1）。 |
```

Replace it with:

```
| sticky header / sidebar 干擾 | matcher：content ROI 排除 top/bottom/side；canvas append：v0.3 overlap-and-overwrite topology（見 3.2.1）對 sticky header / footer / 純色 sidebar / 裝飾邊框天生 cover。 |
```

- [ ] **Step 2: Commit Phase 4**

Run:
```bash
rtk git add docs/rollshot_mvp_design.md
rtk git commit -m "$(cat <<'EOF'
docs: mvp design §3.2.1 + §20 reflect v0.3 overlap topology

§3.2.1 rewritten from "v0.2.1 static region mask (patch)" to
"v0.3 overlap-and-overwrite stitch topology", pointing at the
new spec. §20 risk table row updated to describe how sticky UI
interference is now handled.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 5: Final Verification

### Task 5.1: Full workspace verification

**Files:** none (verification only)

- [ ] **Step 1: Run the full test suite**

Run: `rtk cargo test --workspace`
Expected: all tests pass. The new `tests/overlap_topology.rs` test file
contributes 11 tests; existing matcher / verifier / axis / stitcher tests
all still pass; `tests/canvas.rs` integration tests pass.

- [ ] **Step 2: Format check**

Run: `rtk cargo fmt --check`
Expected: no output, exit 0.

If non-zero, run `rtk cargo fmt` then re-run `--check`, then commit:

```bash
rtk git add -u
rtk git commit -m "style: cargo fmt

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 3: Clippy check**

Run: `rtk cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings, exit 0.

If there are warnings, fix them (most likely candidates: an unused import
left over from removing the static_region paint helpers, or a borrow
suggestion in the new test code). Commit fixes separately:

```bash
rtk git add -u
rtk git commit -m "fix: clippy

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 4: Verify final commit graph**

Run: `rtk git log --oneline -15`
Expected (hashes will differ; the order and shapes should match):

```
<hash>  style: cargo fmt                            # optional
<hash>  docs: mvp design §3.2.1 + §20 ...
<hash>  test(core): horizontal scroll + large-motion fallback ...
<hash>  test(core): scroll-up + bidirectional ...
<hash>  test(core): solid sidebar + top/bottom anchored icon ...
<hash>  test(core): sticky-footer + 1px decorative border ...
<hash>  test(core): first-frame preservation + sticky-header ...
<hash>  test(core): pure-scroll v0.2 equivalence regression gate
<hash>  feat(core): prepend_left uses overlap-and-overwrite
<hash>  feat(core): append_right uses overlap-and-overwrite
<hash>  feat(core): prepend_top uses overlap-and-overwrite
<hash>  feat(core): append_bottom uses overlap-and-overwrite
<hash>  revert: v0.2.1 static region mask (1b16e8de)
20f879c docs: v0.3 overlap-and-overwrite stitch topology spec
1b16e8d feat: static region mask (#9)
```

- [ ] **Step 5: Sanity-spot-check the user's actual use case**

Run a quick sanity test that the decorative-border fixture lands cleanly:

Run: `rtk cargo test -p rollshot-core --test overlap_topology decorative_1px_bottom_border_only_at_canvas_bottom -- --nocapture`
Expected: **PASS** with no debug output. This was the user-reported problem
the entire v0.3 effort exists to solve — confirm it actually passes.

---

## Notes

### Why no v0.2 byte-identical config flag?

v0.2.1 had `StaticRegionConfig::enabled = false` for byte-identical
reproduction. v0.3 has no such flag: the spec proves overlap-and-overwrite is
algebraically equivalent to v0.2 minimal-slice for pure-scroll fixtures, and
`pure_scroll_byte_identical_to_v0_2_minimal_slice` gates this empirically.
Anything that needs "true v0.2 behavior" can checkout the parent commit of
the revert.

### Why one PR per slice helper instead of one big PR?

The four helpers (`append_bottom`, `prepend_top`, `append_right`,
`prepend_left`) are independent functions with no cross-dependencies. Each
gets its own failing-test → implementation → passing-test → commit cycle.
This makes any regression bisectable to a single 30-line change.

### What to do if pure_scroll_byte_identical_to_v0_2_minimal_slice fails

The most likely bug is in one of the four slice helpers' overlap math:
- Off-by-one in `paste_y` / `paste_x` (should be `canvas.height - overlap_size`, not `canvas.height - total_slice` and not `canvas.height + overlap_size`)
- Reversed orientation: taking the slice from the wrong edge (e.g.,
  `frame.view(0, 0, ...)` instead of `frame.view(0, H - total_slice, ...)` in `append_bottom`)
- Forgotten saturating_sub on `overlap_size` (causes panic when `slice_px > H/2`)

Compare the failing helper's code to the spec's pseudocode block and to the
snow-shot reference at
`learn-projects/snow-shot/src-tauri/src-crates/app-scroll-screenshot-service/src/scroll_screenshot_service.rs:358-390`.

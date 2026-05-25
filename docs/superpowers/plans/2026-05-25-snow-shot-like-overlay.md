# Snow-Shot-Like Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current app-workbench capture UI with a direct Snow-Shot-like overlay flow for selecting a crop, stitching, previewing progress safely, and saving the result.

**Architecture:** Keep the existing Rust capture/stitching session as the backend. Replace the frontend workbench with focused overlay components and pure geometry helpers. Add backend capability plumbing so the frontend only renders image preview inside the selected crop when overlay exclusion is verified.

**Tech Stack:** Tauri 2, Rust, React 19, TypeScript, Vitest, CSS, existing `rollshot-capture` and `rollshot-core` crates.

---

## File Structure

Create:

- `crates/rollshot-app/src/overlay/placement.ts`  
  Pure preview-placement helper. It decides whether the live stitch preview is outside the crop, inside the crop, or replaced by status-only.

- `crates/rollshot-app/src/overlay/placement.test.ts`  
  Unit tests for placement choices and safe fallback behavior.

- `crates/rollshot-app/src/overlay/selection.ts`  
  Pure overlay-coordinate to source-coordinate conversion helper.

- `crates/rollshot-app/src/overlay/selection.test.ts`  
  Unit tests for HiDPI and overlay coordinate conversion.

- `crates/rollshot-app/src/components/CaptureOverlay.tsx`  
  Top-level overlay state renderer and polling owner.

- `crates/rollshot-app/src/components/SelectionLayer.tsx`  
  Fullscreen Snow-Shot-like selection layer: dim mask, crosshair cursor, auxiliary lines, drag-to-select.

- `crates/rollshot-app/src/components/SelectionLayer.test.tsx`  
  Interaction tests for drag-to-select and `Esc` cancel.

- `crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx`  
  Live stitch preview/status renderer using placement output.

- `crates/rollshot-app/src/components/AdaptiveStitchPreview.test.tsx`  
  Tests that unsafe placements render status-only.

- `crates/rollshot-app/src/components/OverlayToolbar.tsx`  
  Minimal stop/save/close controls for stitching and done states.

- `crates/rollshot-app/src-tauri/src/overlay.rs`  
  Backend overlay-exclusion capability and overlay-window setup helpers.

Modify:

- `crates/rollshot-app/src/App.tsx`  
  Replace app shell with `CaptureOverlay`.

- `crates/rollshot-app/src/App.css`  
  Replace workbench CSS with transparent overlay styling.

- `crates/rollshot-app/src/api/capture.ts`  
  Add `OverlayExclusion` type and `overlayExclusion()` command wrapper.

- `crates/rollshot-app/src/api/capture.test.ts`  
  Test the new API wrapper.

- `crates/rollshot-app/src-tauri/src/commands.rs`  
  Add `overlay_exclusion` command.

- `crates/rollshot-app/src-tauri/src/lib.rs`  
  Register overlay module, configure the overlay window during setup, and register the new command.

- `crates/rollshot-app/src-tauri/src/session.rs`  
  Expose overlay exclusion state through `SharedSession`.

- `crates/rollshot-app/src-tauri/tauri.conf.json`  
  Configure the main window as transparent, undecorated, always-on-top overlay.

Keep:

- `crates/rollshot-app/src/components/RegionOverlay.tsx` and its test can remain temporarily unused until a cleanup task removes them. Do not delete them in the first pass unless the replacement is fully verified.

---

### Task 1: Add Overlay Preview Placement Helper

**Files:**

- Create: `crates/rollshot-app/src/overlay/placement.ts`
- Create: `crates/rollshot-app/src/overlay/placement.test.ts`

- [ ] **Step 1: Write failing placement tests**

Create `crates/rollshot-app/src/overlay/placement.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { choosePreviewPlacement, type OverlayExclusion } from './placement'

const bounds = { left: 0, top: 0, width: 1000, height: 700 }
const preview = { width: 180, height: 260 }

describe('choosePreviewPlacement', () => {
  it('chooses right when the preview fits beside the region', () => {
    expect(
      choosePreviewPlacement({
        bounds,
        region: { left: 120, top: 90, width: 300, height: 360 },
        preview,
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 432, top: 90, width: 180, height: 260 },
    })
  })

  it('chooses left when right does not fit', () => {
    expect(
      choosePreviewPlacement({
        bounds,
        region: { left: 720, top: 80, width: 240, height: 300 },
        preview,
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'left',
      rect: { left: 528, top: 80, width: 180, height: 260 },
    })
  })

  it('chooses below when horizontal sides do not fit', () => {
    expect(
      choosePreviewPlacement({
        bounds,
        region: { left: 120, top: 90, width: 780, height: 220 },
        preview,
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'bottom',
      rect: { left: 120, top: 322, width: 180, height: 260 },
    })
  })

  it('uses inside preview only when overlay exclusion is verified', () => {
    expect(
      choosePreviewPlacement({
        bounds,
        region: { left: 0, top: 0, width: 1000, height: 700 },
        preview,
        overlayExclusion: 'verified',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'inside',
      rect: { left: 808, top: 12, width: 180, height: 260 },
    })
  })

  it.each<OverlayExclusion>(['unsupported', 'unknown'])(
    'uses status-only for full-screen crops when exclusion is %s',
    (overlayExclusion) => {
      expect(
        choosePreviewPlacement({
          bounds,
          region: { left: 0, top: 0, width: 1000, height: 700 },
          preview,
          overlayExclusion,
          gap: 12,
        }),
      ).toEqual({ mode: 'status' })
    },
  )
})
```

- [ ] **Step 2: Run the failing placement tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- overlay/placement.test.ts
```

Expected: FAIL because `src/overlay/placement.ts` does not exist.

- [ ] **Step 3: Implement the minimal placement helper**

Create `crates/rollshot-app/src/overlay/placement.ts`:

```ts
export type OverlayExclusion = 'verified' | 'unsupported' | 'unknown'

export type OverlayRect = {
  left: number
  top: number
  width: number
  height: number
}

export type PreviewSize = {
  width: number
  height: number
}

export type PreviewPlacement =
  | {
      mode: 'image'
      side: 'right' | 'left' | 'bottom' | 'top' | 'inside'
      rect: OverlayRect
    }
  | { mode: 'status' }

type PlacementInput = {
  bounds: OverlayRect
  region: OverlayRect
  preview: PreviewSize
  overlayExclusion: OverlayExclusion
  gap?: number
}

export function choosePreviewPlacement({
  bounds,
  region,
  preview,
  overlayExclusion,
  gap = 12,
}: PlacementInput): PreviewPlacement {
  const candidates: Array<PreviewPlacement & { mode: 'image' }> = [
    {
      mode: 'image',
      side: 'right',
      rect: {
        left: region.left + region.width + gap,
        top: clamp(region.top, bounds.top, bounds.top + bounds.height - preview.height),
        width: preview.width,
        height: preview.height,
      },
    },
    {
      mode: 'image',
      side: 'left',
      rect: {
        left: region.left - preview.width - gap,
        top: clamp(region.top, bounds.top, bounds.top + bounds.height - preview.height),
        width: preview.width,
        height: preview.height,
      },
    },
    {
      mode: 'image',
      side: 'bottom',
      rect: {
        left: clamp(region.left, bounds.left, bounds.left + bounds.width - preview.width),
        top: region.top + region.height + gap,
        width: preview.width,
        height: preview.height,
      },
    },
    {
      mode: 'image',
      side: 'top',
      rect: {
        left: clamp(region.left, bounds.left, bounds.left + bounds.width - preview.width),
        top: region.top - preview.height - gap,
        width: preview.width,
        height: preview.height,
      },
    },
  ]

  const outside = candidates.find((candidate) => fits(bounds, candidate.rect))
  if (outside) {
    return outside
  }

  if (overlayExclusion === 'verified') {
    return {
      mode: 'image',
      side: 'inside',
      rect: {
        left: clamp(
          region.left + region.width - preview.width - gap,
          bounds.left,
          bounds.left + bounds.width - preview.width,
        ),
        top: clamp(region.top + gap, bounds.top, bounds.top + bounds.height - preview.height),
        width: preview.width,
        height: preview.height,
      },
    }
  }

  return { mode: 'status' }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

function fits(bounds: OverlayRect, rect: OverlayRect): boolean {
  return (
    rect.left >= bounds.left &&
    rect.top >= bounds.top &&
    rect.left + rect.width <= bounds.left + bounds.width &&
    rect.top + rect.height <= bounds.top + bounds.height
  )
}
```

- [ ] **Step 4: Run the placement tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- overlay/placement.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit placement helper**

Run:

```bash
rtk git add crates/rollshot-app/src/overlay/placement.ts crates/rollshot-app/src/overlay/placement.test.ts
rtk git commit -m "feat(app): add overlay preview placement helper"
```

---

### Task 2: Add Overlay Selection Coordinate Helper

**Files:**

- Create: `crates/rollshot-app/src/overlay/selection.ts`
- Create: `crates/rollshot-app/src/overlay/selection.test.ts`

- [ ] **Step 1: Write failing selection tests**

Create `crates/rollshot-app/src/overlay/selection.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { overlayRectToSourceRegion } from './selection'

describe('overlayRectToSourceRegion', () => {
  it('maps overlay CSS pixels to source pixels', () => {
    expect(
      overlayRectToSourceRegion(
        { left: 50, top: 20, width: 200, height: 100 },
        {
          overlayWidth: 500,
          overlayHeight: 250,
          sourceWidth: 1000,
          sourceHeight: 500,
        },
      ),
    ).toEqual({ x: 100, y: 40, width: 400, height: 200 })
  })

  it('floors origin and ceilings far edge for fractional HiDPI coordinates', () => {
    expect(
      overlayRectToSourceRegion(
        { left: 10.25, top: 4.5, width: 20.25, height: 10.25 },
        {
          overlayWidth: 333,
          overlayHeight: 222,
          sourceWidth: 1000,
          sourceHeight: 666,
        },
      ),
    ).toEqual({ x: 30, y: 13, width: 62, height: 32 })
  })

  it('clamps to source bounds', () => {
    expect(
      overlayRectToSourceRegion(
        { left: -20, top: 240, width: 120, height: 80 },
        {
          overlayWidth: 500,
          overlayHeight: 250,
          sourceWidth: 1000,
          sourceHeight: 500,
        },
      ),
    ).toEqual({ x: 0, y: 480, width: 200, height: 20 })
  })
})
```

- [ ] **Step 2: Run the failing selection tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- overlay/selection.test.ts
```

Expected: FAIL because `src/overlay/selection.ts` does not exist.

- [ ] **Step 3: Implement the selection helper**

Create `crates/rollshot-app/src/overlay/selection.ts`:

```ts
import { clampSourceRegion, type CssRect, type SourceRegion } from '../region/geometry'

export type OverlaySourceScale = {
  overlayWidth: number
  overlayHeight: number
  sourceWidth: number
  sourceHeight: number
}

export function overlayRectToSourceRegion(
  rect: CssRect,
  scale: OverlaySourceScale,
): SourceRegion {
  const xScale = scale.sourceWidth / scale.overlayWidth
  const yScale = scale.sourceHeight / scale.overlayHeight
  const left = Math.floor(rect.left * xScale)
  const top = Math.floor(rect.top * yScale)
  const right = Math.ceil((rect.left + rect.width) * xScale)
  const bottom = Math.ceil((rect.top + rect.height) * yScale)

  return clampSourceRegion(
    {
      x: left,
      y: top,
      width: right - left,
      height: bottom - top,
    },
    { width: scale.sourceWidth, height: scale.sourceHeight },
  )
}
```

- [ ] **Step 4: Run the selection tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- overlay/selection.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit selection helper**

Run:

```bash
rtk git add crates/rollshot-app/src/overlay/selection.ts crates/rollshot-app/src/overlay/selection.test.ts
rtk git commit -m "feat(app): add overlay selection coordinate helper"
```

---

### Task 3: Expose Overlay Exclusion Capability

**Files:**

- Create: `crates/rollshot-app/src-tauri/src/overlay.rs`
- Modify: `crates/rollshot-app/src-tauri/src/session.rs`
- Modify: `crates/rollshot-app/src-tauri/src/commands.rs`
- Modify: `crates/rollshot-app/src-tauri/src/lib.rs`
- Modify: `crates/rollshot-app/src/api/capture.ts`
- Modify: `crates/rollshot-app/src/api/capture.test.ts`

- [ ] **Step 1: Add Rust tests for conservative capability behavior**

In `crates/rollshot-app/src-tauri/src/session.rs`, extend the test imports:

```rust
use super::{
    encode_preview_png, AppSession, OverlayExclusion, RegionDto, SessionStatus, SharedSession,
};
```

Add these tests inside `mod tests`:

```rust
#[test]
fn shared_session_defaults_overlay_exclusion_to_unknown() {
    let session = SharedSession::new();

    assert_eq!(
        session.overlay_exclusion().expect("overlay exclusion"),
        OverlayExclusion::Unknown
    );
}

#[test]
fn shared_session_can_store_overlay_exclusion_state() {
    let session = SharedSession::new();

    session.set_overlay_exclusion(OverlayExclusion::Unsupported);

    assert_eq!(
        session.overlay_exclusion().expect("overlay exclusion"),
        OverlayExclusion::Unsupported
    );
}
```

- [ ] **Step 2: Run the failing Rust tests**

Run:

```bash
rtk cargo test -p rollshot-app shared_session_defaults_overlay_exclusion_to_unknown shared_session_can_store_overlay_exclusion_state
```

Expected: FAIL because `OverlayExclusion` and the session methods do not exist.

- [ ] **Step 3: Implement backend capability state**

In `crates/rollshot-app/src-tauri/src/session.rs`, add this enum after `RegionDto`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayExclusion {
    Verified,
    Unsupported,
    Unknown,
}
```

Add a field to `SharedSession`:

```rust
overlay_exclusion: Mutex<OverlayExclusion>,
```

Update `SharedSession::new()`:

```rust
overlay_exclusion: Mutex::new(OverlayExclusion::Unknown),
```

Add methods inside `impl SharedSession`:

```rust
pub fn overlay_exclusion(&self) -> Result<OverlayExclusion, String> {
    self.overlay_exclusion
        .lock()
        .map(|state| *state)
        .map_err(|_| "overlay exclusion lock poisoned".to_string())
}

pub fn set_overlay_exclusion(&self, state: OverlayExclusion) {
    if let Ok(mut current) = self.overlay_exclusion.lock() {
        *current = state;
    }
}
```

- [ ] **Step 4: Add conservative native setup helper**

Create `crates/rollshot-app/src-tauri/src/overlay.rs`:

```rust
use crate::session::OverlayExclusion;

#[cfg(target_os = "linux")]
pub fn initial_overlay_exclusion() -> OverlayExclusion {
    OverlayExclusion::Unsupported
}

#[cfg(not(target_os = "linux"))]
pub fn initial_overlay_exclusion() -> OverlayExclusion {
    OverlayExclusion::Unknown
}

pub fn configure_overlay_window(window: &tauri::WebviewWindow) -> OverlayExclusion {
    let _ = window.set_decorations(false);
    let _ = window.set_always_on_top(true);
    let _ = window.set_skip_taskbar(true);
    initial_overlay_exclusion()
}
```

This establishes safe frontend behavior before platform-specific native exclusion is added.

- [ ] **Step 5: Register the backend command and setup**

In `crates/rollshot-app/src-tauri/src/commands.rs`, update the import:

```rust
use crate::session::{DoneImageDto, OverlayExclusion, RegionDto, SessionStatus, SharedSession};
```

Add command:

```rust
#[tauri::command]
pub fn overlay_exclusion(
    session: tauri::State<'_, Arc<SharedSession>>,
) -> Result<OverlayExclusion, String> {
    session.overlay_exclusion()
}
```

In `crates/rollshot-app/src-tauri/src/lib.rs`, add the module:

```rust
mod overlay;
```

Add a setup block before `.invoke_handler(...)`:

```rust
.setup({
    let shared_session = Arc::clone(&shared_session);
    move |app| {
        if let Some(window) = app.get_webview_window("main") {
            let overlay_exclusion = overlay::configure_overlay_window(&window);
            shared_session.set_overlay_exclusion(overlay_exclusion);
        }
        Ok(())
    }
})
```

Register the command:

```rust
commands::overlay_exclusion,
```

- [ ] **Step 6: Add frontend wrapper and test**

In `crates/rollshot-app/src/api/capture.ts`, add:

```ts
export type OverlayExclusion = 'verified' | 'unsupported' | 'unknown'

export async function overlayExclusion(): Promise<OverlayExclusion> {
  return await invoke<OverlayExclusion>('overlay_exclusion')
}
```

In `crates/rollshot-app/src/api/capture.test.ts`, add:

```ts
it('reads overlay exclusion capability', async () => {
  const { overlayExclusion } = await import('./capture')
  invokeMock.mockResolvedValueOnce('unsupported')

  await expect(overlayExclusion()).resolves.toBe('unsupported')
  expect(invokeMock).toHaveBeenCalledWith('overlay_exclusion')
})
```

- [ ] **Step 7: Run capability verification**

Run:

```bash
rtk cargo test -p rollshot-app overlay_exclusion
rtk pnpm --dir crates/rollshot-app test -- api/capture.test.ts
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: all PASS.

- [ ] **Step 8: Commit capability plumbing**

Run:

```bash
rtk git add crates/rollshot-app/src-tauri/src/overlay.rs crates/rollshot-app/src-tauri/src/session.rs crates/rollshot-app/src-tauri/src/commands.rs crates/rollshot-app/src-tauri/src/lib.rs crates/rollshot-app/src/api/capture.ts crates/rollshot-app/src/api/capture.test.ts
rtk git commit -m "feat(app): expose overlay exclusion capability"
```

---

### Task 4: Build the Snow-Shot-Like Selection Layer

**Files:**

- Create: `crates/rollshot-app/src/components/SelectionLayer.tsx`
- Create: `crates/rollshot-app/src/components/SelectionLayer.test.tsx`

- [ ] **Step 1: Write failing SelectionLayer interaction tests**

Create `crates/rollshot-app/src/components/SelectionLayer.test.tsx`:

```tsx
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { SelectionLayer } from './SelectionLayer'

const reactActGlobal = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT: boolean
}
reactActGlobal.IS_REACT_ACT_ENVIRONMENT = true

function pointerEvent(type: string, x: number, y: number) {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientX: x,
    clientY: y,
  })
  Object.defineProperty(event, 'pointerId', { value: 1 })
  return event
}

describe('SelectionLayer', () => {
  let container: HTMLDivElement
  let root: Root
  let rectSpy: ReturnType<typeof vi.spyOn>
  let originalSetPointerCapture: typeof HTMLDivElement.prototype.setPointerCapture | undefined

  beforeEach(() => {
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
    rectSpy = vi.spyOn(HTMLDivElement.prototype, 'getBoundingClientRect').mockImplementation(
      () =>
        ({
          x: 0,
          y: 0,
          left: 0,
          top: 0,
          right: 500,
          bottom: 250,
          width: 500,
          height: 250,
          toJSON: () => ({}),
        }) as DOMRect,
    )
    originalSetPointerCapture = HTMLDivElement.prototype.setPointerCapture
    HTMLDivElement.prototype.setPointerCapture = vi.fn()
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
    rectSpy.mockRestore()
    if (originalSetPointerCapture) {
      HTMLDivElement.prototype.setPointerCapture = originalSetPointerCapture
    } else {
      Reflect.deleteProperty(HTMLDivElement.prototype, 'setPointerCapture')
    }
  })

  it('publishes a source region on drag release', () => {
    const onSelect = vi.fn()

    act(() => {
      root.render(
        <SelectionLayer
          sourceWidth={1000}
          sourceHeight={500}
          selectedRegion={null}
          onSelect={onSelect}
          onCancel={vi.fn()}
        />,
      )
    })

    const layer = container.querySelector('.selection-layer')
    expect(layer).not.toBeNull()

    act(() => {
      layer?.dispatchEvent(pointerEvent('pointerdown', 50, 25))
      layer?.dispatchEvent(pointerEvent('pointermove', 250, 125))
      layer?.dispatchEvent(pointerEvent('pointerup', 250, 125))
    })

    expect(onSelect).toHaveBeenLastCalledWith({
      x: 100,
      y: 50,
      width: 400,
      height: 200,
    })
    expect(container.querySelector('.selection-box')).not.toBeNull()
  })

  it('ignores tiny selections', () => {
    const onSelect = vi.fn()

    act(() => {
      root.render(
        <SelectionLayer
          sourceWidth={1000}
          sourceHeight={500}
          selectedRegion={null}
          onSelect={onSelect}
          onCancel={vi.fn()}
        />,
      )
    })

    const layer = container.querySelector('.selection-layer')
    act(() => {
      layer?.dispatchEvent(pointerEvent('pointerdown', 50, 25))
      layer?.dispatchEvent(pointerEvent('pointerup', 52, 27))
    })

    expect(onSelect).not.toHaveBeenCalled()
  })

  it('cancels on Escape', () => {
    const onCancel = vi.fn()

    act(() => {
      root.render(
        <SelectionLayer
          sourceWidth={1000}
          sourceHeight={500}
          selectedRegion={null}
          onSelect={vi.fn()}
          onCancel={onCancel}
        />,
      )
    })

    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    })

    expect(onCancel).toHaveBeenCalledOnce()
  })
})
```

- [ ] **Step 2: Run the failing SelectionLayer tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- components/SelectionLayer.test.tsx
```

Expected: FAIL because `SelectionLayer.tsx` does not exist.

- [ ] **Step 3: Implement SelectionLayer**

Create `crates/rollshot-app/src/components/SelectionLayer.tsx`:

```tsx
import { type PointerEvent, useEffect, useMemo, useRef, useState } from 'react'
import { dragToCssRect, sourceRegionToCssRect, type CssRect, type Point, type SourceRegion } from '../region/geometry'
import { overlayRectToSourceRegion } from '../overlay/selection'

type SelectionLayerProps = {
  sourceWidth: number
  sourceHeight: number
  selectedRegion: SourceRegion | null
  onSelect: (region: SourceRegion) => void
  onCancel: () => void
}

export function SelectionLayer({
  sourceWidth,
  sourceHeight,
  selectedRegion,
  onSelect,
  onCancel,
}: SelectionLayerProps) {
  const layerRef = useRef<HTMLDivElement | null>(null)
  const [start, setStart] = useState<Point | null>(null)
  const [draftRect, setDraftRect] = useState<CssRect | null>(null)
  const [cursorPoint, setCursorPoint] = useState<Point | null>(null)

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        onCancel()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onCancel])

  const selectedRect = useMemo(() => {
    const layer = layerRef.current
    if (!selectedRegion || !layer) {
      return null
    }
    const bounds = layer.getBoundingClientRect()
    return sourceRegionToCssRect(selectedRegion, {
      renderedWidth: bounds.width,
      renderedHeight: bounds.height,
      sourceWidth,
      sourceHeight,
    })
  }, [selectedRegion, sourceHeight, sourceWidth])

  const visibleRect = draftRect ?? selectedRect

  function localPoint(event: PointerEvent<HTMLDivElement>): Point {
    const layer = layerRef.current
    if (!layer) {
      return { x: 0, y: 0 }
    }
    const bounds = layer.getBoundingClientRect()
    return {
      x: Math.max(0, Math.min(event.clientX - bounds.left, bounds.width)),
      y: Math.max(0, Math.min(event.clientY - bounds.top, bounds.height)),
    }
  }

  function rectStyle(rect: CssRect) {
    return {
      left: `${rect.left}px`,
      top: `${rect.top}px`,
      width: `${rect.width}px`,
      height: `${rect.height}px`,
    }
  }

  return (
    <div
      ref={layerRef}
      className="selection-layer"
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId)
        const point = localPoint(event)
        setStart(point)
        setDraftRect(dragToCssRect(point, point))
        setCursorPoint(point)
      }}
      onPointerMove={(event) => {
        const point = localPoint(event)
        setCursorPoint(point)
        if (start) {
          setDraftRect(dragToCssRect(start, point))
        }
      }}
      onPointerUp={(event) => {
        if (!start) {
          return
        }
        const nextRect = dragToCssRect(start, localPoint(event))
        setStart(null)
        setDraftRect(nextRect)
        if (nextRect.width < 4 || nextRect.height < 4) {
          setDraftRect(null)
          return
        }
        const bounds = event.currentTarget.getBoundingClientRect()
        onSelect(
          overlayRectToSourceRegion(nextRect, {
            overlayWidth: bounds.width,
            overlayHeight: bounds.height,
            sourceWidth,
            sourceHeight,
          }),
        )
      }}
    >
      <div className="selection-dim" />
      {cursorPoint ? (
        <>
          <div className="selection-guide selection-guide-x" style={{ top: `${cursorPoint.y}px` }} />
          <div className="selection-guide selection-guide-y" style={{ left: `${cursorPoint.x}px` }} />
        </>
      ) : null}
      {visibleRect ? <div className="selection-box" style={rectStyle(visibleRect)} /> : null}
    </div>
  )
}
```

- [ ] **Step 4: Run SelectionLayer tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- components/SelectionLayer.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit SelectionLayer**

Run:

```bash
rtk git add crates/rollshot-app/src/components/SelectionLayer.tsx crates/rollshot-app/src/components/SelectionLayer.test.tsx
rtk git commit -m "feat(app): add overlay selection layer"
```

---

### Task 5: Build Adaptive Preview and Toolbar Components

**Files:**

- Create: `crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx`
- Create: `crates/rollshot-app/src/components/AdaptiveStitchPreview.test.tsx`
- Create: `crates/rollshot-app/src/components/OverlayToolbar.tsx`

- [ ] **Step 1: Write failing AdaptiveStitchPreview tests**

Create `crates/rollshot-app/src/components/AdaptiveStitchPreview.test.tsx`:

```tsx
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { AdaptiveStitchPreview } from './AdaptiveStitchPreview'

describe('AdaptiveStitchPreview', () => {
  it('renders an image when placement allows image preview', () => {
    const html = renderToStaticMarkup(
      <AdaptiveStitchPreview
        imageUrl="blob:stitch"
        status="3 frames"
        placement={{
          mode: 'image',
          side: 'right',
          rect: { left: 120, top: 20, width: 180, height: 260 },
        }}
      />,
    )

    expect(html).toContain('blob:stitch')
    expect(html).toContain('adaptive-stitch-preview')
  })

  it('renders status-only when placement is status', () => {
    const html = renderToStaticMarkup(
      <AdaptiveStitchPreview imageUrl="blob:stitch" status="Stitching live" placement={{ mode: 'status' }} />,
    )

    expect(html).toContain('Stitching live')
    expect(html).not.toContain('blob:stitch')
  })
})
```

- [ ] **Step 2: Run the failing preview tests**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- components/AdaptiveStitchPreview.test.tsx
```

Expected: FAIL because `AdaptiveStitchPreview.tsx` does not exist.

- [ ] **Step 3: Implement AdaptiveStitchPreview**

Create `crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx`:

```tsx
import type { PreviewPlacement } from '../overlay/placement'

type AdaptiveStitchPreviewProps = {
  imageUrl: string | null
  status: string
  placement: PreviewPlacement
}

export function AdaptiveStitchPreview({ imageUrl, status, placement }: AdaptiveStitchPreviewProps) {
  if (placement.mode === 'status' || !imageUrl) {
    return <div className="capture-status">{status}</div>
  }

  return (
    <div
      className={`adaptive-stitch-preview adaptive-stitch-preview-${placement.side}`}
      style={{
        left: `${placement.rect.left}px`,
        top: `${placement.rect.top}px`,
        width: `${placement.rect.width}px`,
        height: `${placement.rect.height}px`,
      }}
    >
      <img src={imageUrl} alt="Stitching preview" draggable={false} />
    </div>
  )
}
```

- [ ] **Step 4: Implement OverlayToolbar**

Create `crates/rollshot-app/src/components/OverlayToolbar.tsx`:

```tsx
import { Save, Square, X } from 'lucide-react'
import { Button } from '@/components/ui/button'

type OverlayToolbarProps = {
  mode: 'stitching' | 'done' | 'failed'
  message: string
  onStop: () => void
  onSave: () => void
  onClose: () => void
}

export function OverlayToolbar({ mode, message, onStop, onSave, onClose }: OverlayToolbarProps) {
  return (
    <div className={`overlay-toolbar overlay-toolbar-${mode}`}>
      <span className="overlay-toolbar-message">{message}</span>
      {mode === 'stitching' ? (
        <Button type="button" size="sm" variant="outline" onClick={onStop}>
          <Square className="size-4" aria-hidden="true" />
          Stop
        </Button>
      ) : null}
      {mode === 'done' ? (
        <Button type="button" size="sm" onClick={onSave}>
          <Save className="size-4" aria-hidden="true" />
          Save
        </Button>
      ) : null}
      <Button type="button" size="sm" variant="ghost" onClick={onClose}>
        <X className="size-4" aria-hidden="true" />
        Close
      </Button>
    </div>
  )
}
```

- [ ] **Step 5: Run component tests and typecheck**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test -- components/AdaptiveStitchPreview.test.tsx
rtk pnpm --dir crates/rollshot-app run typecheck
```

Expected: all PASS.

- [ ] **Step 6: Commit preview/toolbar components**

Run:

```bash
rtk git add crates/rollshot-app/src/components/AdaptiveStitchPreview.tsx crates/rollshot-app/src/components/AdaptiveStitchPreview.test.tsx crates/rollshot-app/src/components/OverlayToolbar.tsx
rtk git commit -m "feat(app): add adaptive stitch preview components"
```

---

### Task 6: Replace App Workbench With CaptureOverlay Flow

**Files:**

- Create: `crates/rollshot-app/src/components/CaptureOverlay.tsx`
- Modify: `crates/rollshot-app/src/App.tsx`

- [ ] **Step 1: Implement CaptureOverlay**

Create `crates/rollshot-app/src/components/CaptureOverlay.tsx`:

```tsx
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { save } from '@tauri-apps/plugin-dialog'
import {
  confirmRegion,
  getFinalPreview,
  getStitchPreview,
  launchOptions,
  overlayExclusion,
  saveImage,
  sessionStatus,
  startCapture,
  startStitching,
  stopCapture,
  stopStitching,
  type InteractiveLaunchOptions,
  type OverlayExclusion,
  type SessionStatus,
} from '../api/capture'
import type { SourceRegion } from '../region/geometry'
import { sourceRegionToCssRect } from '../region/geometry'
import { choosePreviewPlacement } from '../overlay/placement'
import { AdaptiveStitchPreview } from './AdaptiveStitchPreview'
import { OverlayToolbar } from './OverlayToolbar'
import { SelectionLayer } from './SelectionLayer'

const PREVIEW_SIZE = { width: 180, height: 260 }

export function CaptureOverlay() {
  const [status, setStatus] = useState<SessionStatus>({ state: 'idle' })
  const [options, setOptions] = useState<InteractiveLaunchOptions | null>(null)
  const [overlayMode, setOverlayMode] = useState<OverlayExclusion>('unknown')
  const [selectedRegion, setSelectedRegion] = useState<SourceRegion | null>(null)
  const [stitchPreviewUrl, setStitchPreviewUrl] = useState<string | null>(null)
  const [finalPreviewUrl, setFinalPreviewUrl] = useState<string | null>(null)
  const [message, setMessage] = useState('Starting capture')
  const pollInFlightRef = useRef(false)
  const stitchPreviewUrlRef = useRef<string | null>(null)
  const finalPreviewUrlRef = useRef<string | null>(null)

  useEffect(() => {
    stitchPreviewUrlRef.current = stitchPreviewUrl
  }, [stitchPreviewUrl])

  useEffect(() => {
    finalPreviewUrlRef.current = finalPreviewUrl
  }, [finalPreviewUrl])

  useEffect(() => {
    return () => {
      if (stitchPreviewUrlRef.current) URL.revokeObjectURL(stitchPreviewUrlRef.current)
      if (finalPreviewUrlRef.current) URL.revokeObjectURL(finalPreviewUrlRef.current)
    }
  }, [])

  useEffect(() => {
    Promise.all([launchOptions(), overlayExclusion()])
      .then(([loadedOptions, loadedExclusion]) => {
        setOptions(loadedOptions)
        setOverlayMode(loadedExclusion)
        return startCapture(loadedOptions)
      })
      .then(() => setMessage('Select a region'))
      .catch((error) => {
        setStatus({ state: 'failed', message: String(error) })
        setMessage(String(error))
      })
  }, [])

  useEffect(() => {
    const timer = window.setInterval(async () => {
      if (pollInFlightRef.current) return
      pollInFlightRef.current = true
      try {
        const nextStatus = await sessionStatus()
        setStatus(nextStatus)
        if (nextStatus.state === 'stitching') {
          const blob = await getStitchPreview(700)
          if (blob) {
            const nextUrl = URL.createObjectURL(blob)
            setStitchPreviewUrl((oldUrl) => {
              if (oldUrl) URL.revokeObjectURL(oldUrl)
              return nextUrl
            })
          }
        }
      } catch (error) {
        setMessage(String(error))
      } finally {
        pollInFlightRef.current = false
      }
    }, 160)

    return () => window.clearInterval(timer)
  }, [])

  const onSelect = useCallback(async (region: SourceRegion) => {
    try {
      setSelectedRegion(region)
      const confirmed = await confirmRegion(region)
      setMessage(`${confirmed.width}x${confirmed.height} selected`)
      await startStitching()
      setMessage('Scroll now')
    } catch (error) {
      setMessage(String(error))
    }
  }, [])

  const onCancel = useCallback(async () => {
    try {
      await stopCapture()
    } finally {
      window.close()
    }
  }, [])

  const onStop = useCallback(async () => {
    try {
      const done = await stopStitching()
      setMessage(`Stitched ${done.image_width}x${done.image_height}`)
      const blob = await getFinalPreview(1400)
      if (blob) {
        const nextUrl = URL.createObjectURL(blob)
        setFinalPreviewUrl((oldUrl) => {
          if (oldUrl) URL.revokeObjectURL(oldUrl)
          return nextUrl
        })
      }
    } catch (error) {
      setMessage(String(error))
    }
  }, [])

  const onSave = useCallback(async () => {
    try {
      const selected = await save({
        title: 'Save stitched PNG',
        defaultPath: 'rollshot.png',
        filters: [{ name: 'PNG image', extensions: ['png'] }],
      })
      if (!selected) return
      const done = await saveImage(selected)
      setMessage(done.output_path ? `Saved ${done.output_path}` : 'Saved image')
    } catch (error) {
      setMessage(String(error))
    }
  }, [])

  const activeRegion = selectedRegion ?? (status.state === 'stitching' ? status.region : null)
  const sourceWidth = status.state === 'previewing' || status.state === 'stitching' ? status.frame_width : 1
  const sourceHeight = status.state === 'previewing' || status.state === 'stitching' ? status.frame_height : 1
  const canSelect = status.state === 'previewing' || status.state === 'stitching'

  const placement = useMemo(() => {
    if (!activeRegion) {
      return { mode: 'status' } as const
    }
    const bounds = { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight }
    const regionRect = sourceRegionToCssRect(activeRegion, {
      renderedWidth: bounds.width,
      renderedHeight: bounds.height,
      sourceWidth,
      sourceHeight,
    })
    return choosePreviewPlacement({
      bounds,
      region: regionRect,
      preview: PREVIEW_SIZE,
      overlayExclusion: overlayMode,
    })
  }, [activeRegion, overlayMode, sourceHeight, sourceWidth])

  const toolbarMode = status.state === 'done' ? 'done' : status.state === 'failed' ? 'failed' : 'stitching'
  const stats =
    status.state === 'stitching'
      ? `${status.stats.frame_count} frames - ${status.stats.total_width}x${status.stats.total_height}px`
      : message

  return (
    <main className="capture-overlay">
      {status.state === 'done' && finalPreviewUrl ? (
        <img className="final-overlay-preview" src={finalPreviewUrl} alt="Stitched result" draggable={false} />
      ) : null}
      {canSelect ? (
        <SelectionLayer
          sourceWidth={sourceWidth}
          sourceHeight={sourceHeight}
          selectedRegion={activeRegion}
          onSelect={onSelect}
          onCancel={onCancel}
        />
      ) : status.state !== 'done' ? (
        <div className="selection-layer">
          <div className="selection-dim" />
          <div className="capture-status">{message}</div>
        </div>
      ) : null}
      {status.state === 'stitching' ? (
        <AdaptiveStitchPreview imageUrl={stitchPreviewUrl} status={stats} placement={placement} />
      ) : null}
      {status.state === 'stitching' || status.state === 'done' || status.state === 'failed' ? (
        <OverlayToolbar
          mode={toolbarMode}
          message={status.state === 'failed' ? status.message : stats}
          onStop={onStop}
          onSave={onSave}
          onClose={onCancel}
        />
      ) : null}
    </main>
  )
}
```

- [ ] **Step 2: Replace App with CaptureOverlay**

Replace `crates/rollshot-app/src/App.tsx` with:

```tsx
import { CaptureOverlay } from './components/CaptureOverlay'

export default function App() {
  return <CaptureOverlay />
}
```

- [ ] **Step 3: Run focused frontend checks**

Run:

```bash
rtk pnpm --dir crates/rollshot-app run typecheck
rtk pnpm --dir crates/rollshot-app test
```

Expected: PASS.

- [ ] **Step 4: Commit CaptureOverlay flow**

Run:

```bash
rtk git add crates/rollshot-app/src/components/CaptureOverlay.tsx crates/rollshot-app/src/App.tsx
rtk git commit -m "feat(app): switch to capture overlay flow"
```

---

### Task 7: Apply Overlay Window CSS and Tauri Config

**Files:**

- Modify: `crates/rollshot-app/src/App.css`
- Modify: `crates/rollshot-app/src-tauri/tauri.conf.json`

- [ ] **Step 1: Replace workbench CSS with overlay CSS**

In `crates/rollshot-app/src/App.css`, keep the Tailwind imports/theme variables at the top, then replace app-specific rules from `body` onward with:

```css
body {
  margin: 0;
  min-width: 320px;
  min-height: 100vh;
  background: transparent;
  overflow: hidden;
}

button:not(:disabled),
[role="button"]:not(:disabled) {
  cursor: pointer;
}

#root {
  width: 100vw;
  height: 100vh;
  background: transparent;
}

.capture-overlay {
  position: fixed;
  inset: 0;
  overflow: hidden;
  background: transparent;
  color: #f8fafc;
  user-select: none;
}

.selection-layer {
  position: fixed;
  inset: 0;
  cursor: crosshair;
  touch-action: none;
}

.selection-dim {
  position: absolute;
  inset: 0;
  pointer-events: none;
  background: rgba(0, 0, 0, 0.34);
}

.selection-guide {
  position: absolute;
  pointer-events: none;
  background: rgba(147, 197, 253, 0.48);
}

.selection-guide-x {
  left: 0;
  right: 0;
  height: 1px;
}

.selection-guide-y {
  top: 0;
  bottom: 0;
  width: 1px;
}

.selection-box {
  position: absolute;
  pointer-events: none;
  border: 2px solid #22c55e;
  background: rgba(34, 197, 94, 0.08);
  box-shadow:
    0 0 0 1px rgba(255, 255, 255, 0.72),
    0 0 0 9999px rgba(0, 0, 0, 0.28);
}

.adaptive-stitch-preview {
  position: fixed;
  overflow: hidden;
  border: 1px solid rgba(226, 232, 240, 0.32);
  border-radius: 6px;
  background: rgba(15, 23, 42, 0.9);
  box-shadow: 0 18px 36px rgba(0, 0, 0, 0.32);
}

.adaptive-stitch-preview img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
  background: white;
}

.capture-status {
  position: fixed;
  left: 50%;
  top: 20px;
  transform: translateX(-50%);
  padding: 8px 12px;
  border-radius: 6px;
  background: rgba(15, 23, 42, 0.92);
  color: #f8fafc;
  font-size: 13px;
  box-shadow: 0 14px 28px rgba(0, 0, 0, 0.3);
}

.overlay-toolbar {
  position: fixed;
  left: 50%;
  bottom: 20px;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: calc(100vw - 32px);
  padding: 8px;
  border: 1px solid rgba(226, 232, 240, 0.2);
  border-radius: 6px;
  background: rgba(15, 23, 42, 0.94);
  box-shadow: 0 16px 32px rgba(0, 0, 0, 0.34);
}

.overlay-toolbar-message {
  max-width: 52vw;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: #e2e8f0;
  font-size: 13px;
}

.final-overlay-preview {
  position: fixed;
  inset: 24px;
  width: calc(100vw - 48px);
  height: calc(100vh - 96px);
  object-fit: contain;
  border: 1px solid rgba(226, 232, 240, 0.24);
  border-radius: 6px;
  background: rgba(15, 23, 42, 0.72);
}
```

- [ ] **Step 2: Configure Tauri window as overlay**

In `crates/rollshot-app/src-tauri/tauri.conf.json`, update the `main` window object:

```json
{
  "label": "main",
  "title": "rollshot",
  "width": 1180,
  "height": 780,
  "minWidth": 320,
  "minHeight": 240,
  "resizable": false,
  "fullscreen": false,
  "decorations": false,
  "transparent": true,
  "alwaysOnTop": true,
  "skipTaskbar": true
}
```

- [ ] **Step 3: Run frontend build checks**

Run:

```bash
rtk pnpm --dir crates/rollshot-app run typecheck
rtk pnpm --dir crates/rollshot-app run build
```

Expected: PASS.

- [ ] **Step 4: Commit overlay styling/config**

Run:

```bash
rtk git add crates/rollshot-app/src/App.css crates/rollshot-app/src-tauri/tauri.conf.json
rtk git commit -m "feat(app): style capture window as overlay"
```

---

### Task 8: Add Platform Native Exclusion Best Effort

**Files:**

- Modify: `crates/rollshot-app/src-tauri/Cargo.toml`
- Modify: `crates/rollshot-app/src-tauri/src/overlay.rs`

- [ ] **Step 1: Add Windows dependency**

In `crates/rollshot-app/src-tauri/Cargo.toml`, add:

```toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = ["Win32_Foundation", "Win32_UI_WindowsAndMessaging"] }
```

- [ ] **Step 2: Implement Windows exclusion call**

Replace `configure_overlay_window` in `crates/rollshot-app/src-tauri/src/overlay.rs` with:

```rust
pub fn configure_overlay_window(window: &tauri::WebviewWindow) -> OverlayExclusion {
    let _ = window.set_decorations(false);
    let _ = window.set_always_on_top(true);
    let _ = window.set_skip_taskbar(true);

    platform_overlay_exclusion(window)
}
```

Add platform helpers:

```rust
#[cfg(target_os = "windows")]
fn platform_overlay_exclusion(window: &tauri::WebviewWindow) -> OverlayExclusion {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};

    let Ok(handle) = window.window_handle() else {
        return OverlayExclusion::Unknown;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return OverlayExclusion::Unknown;
    };
    let hwnd = HWND(handle.hwnd.get() as isize);
    let ok = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).as_bool() };
    if ok {
        OverlayExclusion::Verified
    } else {
        OverlayExclusion::Unknown
    }
}

#[cfg(target_os = "linux")]
fn platform_overlay_exclusion(_window: &tauri::WebviewWindow) -> OverlayExclusion {
    OverlayExclusion::Unsupported
}

#[cfg(target_os = "macos")]
fn platform_overlay_exclusion(_window: &tauri::WebviewWindow) -> OverlayExclusion {
    OverlayExclusion::Unknown
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn platform_overlay_exclusion(_window: &tauri::WebviewWindow) -> OverlayExclusion {
    OverlayExclusion::Unknown
}
```

Also add this dependency because the code uses `raw_window_handle` directly:

```toml
raw-window-handle = "0.6"
```

under `[dependencies]`.

- [ ] **Step 3: Run Rust checks**

Run:

```bash
rtk cargo test -p rollshot-app
rtk cargo fmt --check
```

Expected: PASS on the current platform. If the current platform is not Windows, this still validates the Linux/macOS compile path for the active target.

- [ ] **Step 4: Commit native exclusion best effort**

Run:

```bash
rtk git add crates/rollshot-app/src-tauri/Cargo.toml crates/rollshot-app/src-tauri/src/overlay.rs
rtk git commit -m "feat(app): add native overlay exclusion best effort"
```

---

### Task 9: Final Verification and Cleanup

**Files:**

- Modify only files needed to fix verification failures.

- [ ] **Step 1: Run full frontend verification**

Run:

```bash
rtk pnpm --dir crates/rollshot-app test
rtk pnpm --dir crates/rollshot-app run typecheck
rtk pnpm --dir crates/rollshot-app run build
```

Expected: all PASS.

- [ ] **Step 2: Run Rust verification**

Run:

```bash
rtk cargo test
rtk cargo fmt --check
```

Expected: all PASS.

- [ ] **Step 3: Run clippy if the previous checks pass**

Run:

```bash
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS. If clippy reports pre-existing unrelated warnings, document the exact warnings and do not fix unrelated code.

- [ ] **Step 4: Manual overlay smoke test**

Run the app through the existing capture launcher command used by this branch. If the launcher command is not known, run:

```bash
rtk cargo run -p rollshot -- capture --help
```

Then launch capture with the command shown by help.

Expected manual behavior:

- no app workbench or Start panel appears
- capture starts directly in overlay mode
- background is dimmed
- cursor is crosshair over the overlay
- auxiliary lines follow pointer movement
- dragging creates a selected crop box
- mouseup starts stitching
- live stitch preview appears outside the crop when space allows
- full-screen crop on Linux shows status-only when no outside placement exists
- Stop produces a final preview
- Save writes a PNG

- [ ] **Step 5: Confirm no verification-only changes remain**

Run:

```bash
rtk git status --short
```

Expected: no output. If there is output, stop and review the changed paths
before deciding whether they belong in the preceding task commits.

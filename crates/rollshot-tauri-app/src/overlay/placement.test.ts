import { describe, expect, it } from 'vitest'
import {
  chooseDynamicPreviewPlacement,
  choosePreviewPlacement,
  fitPreviewSizeToRegion,
  type OverlayExclusion,
} from './placement'

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

describe('chooseDynamicPreviewPlacement', () => {
  it('uses fixed width and a short height when scaled content is short', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 900 },
        region: { left: 100, top: 100, width: 400, height: 300 },
        previewWidth: 280,
        content: { width: 400, height: 200 },
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 512, top: 100, width: 280, height: 140 },
      preview: { width: 280, height: 140 },
    })
  })

  it('grows height with scaled content and caps at crop height', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 900 },
        region: { left: 100, top: 100, width: 400, height: 300 },
        previewWidth: 280,
        content: { width: 400, height: 600 },
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 512, top: 100, width: 280, height: 300 },
      preview: { width: 280, height: 300 },
    })
  })

  it('caps side preview height to the available band before choosing placement', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 700 },
        region: { left: 100, top: 450, width: 400, height: 300 },
        previewWidth: 280,
        content: { width: 400, height: 600 },
        overlayExclusion: 'unsupported',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'right',
      rect: { left: 512, top: 450, width: 280, height: 250 },
      preview: { width: 280, height: 250 },
    })
  })

  it('uses verified inside placement with growing preview height', () => {
    expect(
      chooseDynamicPreviewPlacement({
        bounds: { left: 0, top: 0, width: 1000, height: 700 },
        region: { left: 0, top: 0, width: 1000, height: 700 },
        previewWidth: 280,
        content: { width: 1000, height: 1400 },
        overlayExclusion: 'verified',
        gap: 12,
      }),
    ).toEqual({
      mode: 'image',
      side: 'inside',
      rect: { left: 708, top: 12, width: 280, height: 392 },
      preview: { width: 280, height: 392 },
    })
  })
})

describe('fitPreviewSizeToRegion', () => {
  it('keeps a wide crop from filling a tall preview box', () => {
    expect(
      fitPreviewSizeToRegion({
        region: { width: 2400, height: 900 },
        maxPreview: { width: 180, height: 260 },
      }),
    ).toEqual({ width: 180, height: 68 })
  })

  it('reduces width for a tall crop instead of letterboxing horizontally', () => {
    expect(
      fitPreviewSizeToRegion({
        region: { width: 400, height: 1200 },
        maxPreview: { width: 180, height: 260 },
      }),
    ).toEqual({ width: 87, height: 260 })
  })

  it('sizes a wide crop to max width with 280px dynamic cap', () => {
    expect(
      fitPreviewSizeToRegion({
        region: { width: 2400, height: 900 },
        maxPreview: { width: 280, height: 900 },
      }),
    ).toEqual({ width: 280, height: 105 })
  })

  it('sizes a tall crop to max width with 280px dynamic cap', () => {
    expect(
      fitPreviewSizeToRegion({
        region: { width: 400, height: 1200 },
        maxPreview: { width: 280, height: 1200 },
      }),
    ).toEqual({ width: 280, height: 840 })
  })
})

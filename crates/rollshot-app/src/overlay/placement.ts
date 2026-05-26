import type { OverlayExclusion } from '../api/capture'

export type { OverlayExclusion }

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

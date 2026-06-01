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

type RegionSize = {
  width: number
  height: number
}

type FitPreviewInput = {
  region: RegionSize
  maxPreview: PreviewSize
}

export function fitPreviewSizeToRegion({ region, maxPreview }: FitPreviewInput): PreviewSize {
  const maxWidth = Math.max(1, Math.floor(maxPreview.width))
  const maxHeight = Math.max(1, Math.floor(maxPreview.height))
  const regionWidth = Math.max(1, region.width)
  const regionHeight = Math.max(1, region.height)
  const aspect = regionWidth / regionHeight
  const maxAspect = maxWidth / maxHeight

  if (aspect >= maxAspect) {
    return {
      width: maxWidth,
      height: Math.max(1, Math.min(maxHeight, Math.round(maxWidth / aspect))),
    }
  }

  return {
    width: Math.max(1, Math.min(maxWidth, Math.round(maxHeight * aspect))),
    height: maxHeight,
  }
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

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

export type DynamicPreviewPlacement =
  | {
      mode: 'image'
      side: 'right' | 'left' | 'bottom' | 'top' | 'inside'
      rect: OverlayRect
      preview: PreviewSize
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

type ContentSize = {
  width: number
  height: number
}

type DynamicPlacementInput = {
  bounds: OverlayRect
  region: OverlayRect
  previewWidth: number
  content: ContentSize
  overlayExclusion: OverlayExclusion
  gap?: number
}

function dynamicPreviewSize(input: {
  content: ContentSize
  previewWidth: number
  maxWidth: number
  maxHeight: number
  cropHeight: number
}): PreviewSize {
  const width = Math.max(1, Math.floor(Math.min(input.previewWidth, input.maxWidth)))
  const contentWidth = Math.max(1, input.content.width)
  const contentHeight = Math.max(1, input.content.height)
  const scaledHeight = Math.max(1, Math.round((contentHeight * width) / contentWidth))
  const height = Math.max(
    1,
    Math.floor(Math.min(scaledHeight, input.cropHeight, input.maxHeight)),
  )
  return { width, height }
}

export function chooseDynamicPreviewPlacement({
  bounds,
  region,
  previewWidth,
  content,
  overlayExclusion,
  gap = 12,
}: DynamicPlacementInput): DynamicPreviewPlacement {
  const boundsRight = bounds.left + bounds.width
  const boundsBottom = bounds.top + bounds.height
  const regionRight = region.left + region.width
  const regionBottom = region.top + region.height

  const sides: Array<{
    side: 'right' | 'left' | 'bottom' | 'top'
    availWidth: number
    availHeight: number
  }> = [
    {
      side: 'right',
      availWidth: boundsRight - regionRight - gap,
      availHeight: boundsBottom - region.top,
    },
    {
      side: 'left',
      availWidth: region.left - bounds.left - gap,
      availHeight: boundsBottom - region.top,
    },
    {
      side: 'bottom',
      availWidth: boundsRight - region.left,
      availHeight: boundsBottom - regionBottom - gap,
    },
    {
      side: 'top',
      availWidth: boundsRight - region.left,
      availHeight: region.top - bounds.top - gap,
    },
  ]

  for (const { side, availWidth, availHeight } of sides) {
    const preview = dynamicPreviewSize({
      content,
      previewWidth,
      maxWidth: availWidth,
      maxHeight: availHeight,
      cropHeight: region.height,
    })

    let rect: OverlayRect
    if (side === 'right') {
      rect = {
        left: regionRight + gap,
        top: clamp(region.top, bounds.top, boundsBottom - preview.height),
        width: preview.width,
        height: preview.height,
      }
    } else if (side === 'left') {
      rect = {
        left: region.left - preview.width - gap,
        top: clamp(region.top, bounds.top, boundsBottom - preview.height),
        width: preview.width,
        height: preview.height,
      }
    } else if (side === 'bottom') {
      rect = {
        left: clamp(region.left, bounds.left, boundsRight - preview.width),
        top: regionBottom + gap,
        width: preview.width,
        height: preview.height,
      }
    } else {
      rect = {
        left: clamp(region.left, bounds.left, boundsRight - preview.width),
        top: region.top - preview.height - gap,
        width: preview.width,
        height: preview.height,
      }
    }

    if (fits(bounds, rect)) {
      return { mode: 'image', side, rect, preview }
    }
  }

  if (overlayExclusion === 'verified') {
    const insideAvailWidth = region.width - gap * 2
    const insideAvailHeight = region.height - gap * 2

    const preview = dynamicPreviewSize({
      content,
      previewWidth,
      maxWidth: insideAvailWidth,
      maxHeight: insideAvailHeight,
      cropHeight: region.height,
    })

    return {
      mode: 'image',
      side: 'inside',
      rect: {
        left: clamp(
          regionRight - preview.width - gap,
          bounds.left,
          boundsRight - preview.width,
        ),
        top: clamp(region.top + gap, bounds.top, boundsBottom - preview.height),
        width: preview.width,
        height: preview.height,
      },
      preview,
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

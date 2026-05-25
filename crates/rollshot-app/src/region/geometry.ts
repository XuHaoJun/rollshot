export type Point = {
  x: number
  y: number
}

export type CssRect = {
  left: number
  top: number
  width: number
  height: number
}

export type SourceRegion = {
  x: number
  y: number
  width: number
  height: number
}

export type PreviewScale = {
  scaleX: number
  scaleY: number
  sourceOriginX: number
  sourceOriginY: number
  sourceWidth: number
  sourceHeight: number
}

export type SourceSize = {
  width: number
  height: number
}

export function dragToCssRect(start: Point, current: Point): CssRect {
  const left = Math.min(start.x, current.x)
  const top = Math.min(start.y, current.y)
  return {
    left,
    top,
    width: Math.abs(current.x - start.x),
    height: Math.abs(current.y - start.y),
  }
}

export function previewScaleFromRendered(input: {
  renderedWidth: number
  renderedHeight: number
  sourceWidth: number
  sourceHeight: number
}): PreviewScale {
  return {
    scaleX: input.sourceWidth / input.renderedWidth,
    scaleY: input.sourceHeight / input.renderedHeight,
    sourceOriginX: 0,
    sourceOriginY: 0,
    sourceWidth: input.sourceWidth,
    sourceHeight: input.sourceHeight,
  }
}

export function cssRectToSourceRegion(
  rect: CssRect,
  scale: PreviewScale,
): SourceRegion {
  const left = Math.floor(scale.sourceOriginX + rect.left * scale.scaleX)
  const top = Math.floor(scale.sourceOriginY + rect.top * scale.scaleY)
  const right = Math.ceil(scale.sourceOriginX + (rect.left + rect.width) * scale.scaleX)
  const bottom = Math.ceil(scale.sourceOriginY + (rect.top + rect.height) * scale.scaleY)

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

export function sourceRegionToCssRect(
  region: SourceRegion,
  scale: PreviewScale,
): CssRect {
  return {
    left: (region.x - scale.sourceOriginX) / scale.scaleX,
    top: (region.y - scale.sourceOriginY) / scale.scaleY,
    width: region.width / scale.scaleX,
    height: region.height / scale.scaleY,
  }
}

export function clampSourceRegion(
  region: SourceRegion,
  source: SourceSize,
): SourceRegion {
  const x = Math.max(0, Math.min(Math.round(region.x), source.width))
  const y = Math.max(0, Math.min(Math.round(region.y), source.height))
  const right = Math.max(x, Math.min(Math.round(region.x + region.width), source.width))
  const bottom = Math.max(y, Math.min(Math.round(region.y + region.height), source.height))
  return {
    x,
    y,
    width: right - x,
    height: bottom - y,
  }
}

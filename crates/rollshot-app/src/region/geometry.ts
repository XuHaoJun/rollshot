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
  renderedWidth: number
  renderedHeight: number
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

export function cssRectToSourceRegion(
  rect: CssRect,
  scale: PreviewScale,
): SourceRegion {
  const xScale = scale.sourceWidth / scale.renderedWidth
  const yScale = scale.sourceHeight / scale.renderedHeight
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

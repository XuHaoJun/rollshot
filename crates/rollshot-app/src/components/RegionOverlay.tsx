import { type PointerEvent, useMemo, useRef, useState } from 'react'
import {
  cssRectToSourceRegion,
  dragToCssRect,
  type CssRect,
  type Point,
  type SourceRegion,
} from '../region/geometry'

type RegionOverlayProps = {
  imageUrl: string
  sourceWidth: number
  sourceHeight: number
  onRegionChange: (region: SourceRegion | null) => void
}

export function RegionOverlay({
  imageUrl,
  sourceWidth,
  sourceHeight,
  onRegionChange,
}: RegionOverlayProps) {
  const imageRef = useRef<HTMLImageElement | null>(null)
  const [start, setStart] = useState<Point | null>(null)
  const [rect, setRect] = useState<CssRect | null>(null)

  const overlayStyle = useMemo(() => {
    if (!rect) {
      return undefined
    }
    return {
      left: `${rect.left}px`,
      top: `${rect.top}px`,
      width: `${rect.width}px`,
      height: `${rect.height}px`,
    }
  }, [rect])

  function localPoint(event: PointerEvent<HTMLDivElement>): Point {
    const bounds = event.currentTarget.getBoundingClientRect()
    return {
      x: event.clientX - bounds.left,
      y: event.clientY - bounds.top,
    }
  }

  function publishRegion(nextRect: CssRect | null) {
    const image = imageRef.current
    if (!image || !nextRect || nextRect.width < 4 || nextRect.height < 4) {
      onRegionChange(null)
      return
    }

    onRegionChange(
      cssRectToSourceRegion(nextRect, {
        renderedWidth: image.clientWidth,
        renderedHeight: image.clientHeight,
        sourceWidth,
        sourceHeight,
      }),
    )
  }

  return (
    <div
      className="preview-wrap"
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId)
        const point = localPoint(event)
        setStart(point)
        const nextRect = dragToCssRect(point, point)
        setRect(nextRect)
        publishRegion(nextRect)
      }}
      onPointerMove={(event) => {
        if (!start) {
          return
        }
        const nextRect = dragToCssRect(start, localPoint(event))
        setRect(nextRect)
        publishRegion(nextRect)
      }}
      onPointerUp={(event) => {
        if (!start) {
          return
        }
        const nextRect = dragToCssRect(start, localPoint(event))
        setStart(null)
        setRect(nextRect)
        publishRegion(nextRect)
      }}
    >
      <img
        ref={imageRef}
        className="preview-image"
        src={imageUrl}
        alt="Live capture preview"
        draggable={false}
      />
      <div className="selection-dim" />
      {overlayStyle ? <div className="selection-box" style={overlayStyle} /> : null}
    </div>
  )
}

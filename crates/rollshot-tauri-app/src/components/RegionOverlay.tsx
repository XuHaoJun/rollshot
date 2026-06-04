import { type PointerEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  cssRectToSourceRegion,
  dragToCssRect,
  previewScaleFromRendered,
  type CssRect,
  type Point,
  type SourceRegion,
  sourceRegionToCssRect,
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
  const [selectedRegion, setSelectedRegion] = useState<SourceRegion | null>(null)
  const [renderedSize, setRenderedSize] = useState<{ width: number; height: number } | null>(null)

  const updateRenderedSize = useCallback(() => {
    const image = imageRef.current
    if (!image) {
      return
    }

    const bounds = image.getBoundingClientRect()
    if (bounds.width <= 0 || bounds.height <= 0) {
      return
    }

    setRenderedSize((current) => {
      if (current?.width === bounds.width && current.height === bounds.height) {
        return current
      }
      return { width: bounds.width, height: bounds.height }
    })
  }, [])

  useEffect(() => {
    updateRenderedSize()
    const image = imageRef.current
    const observer =
      image && typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(updateRenderedSize)
        : null

    if (image && observer) {
      observer.observe(image)
    }
    window.addEventListener('resize', updateRenderedSize)

    return () => {
      observer?.disconnect()
      window.removeEventListener('resize', updateRenderedSize)
    }
  }, [imageUrl, updateRenderedSize])

  const overlayStyle = useMemo(() => {
    if (!selectedRegion || !renderedSize) {
      return undefined
    }
    const rect = sourceRegionToCssRect(
      selectedRegion,
      previewScaleFromRendered({
        renderedWidth: renderedSize.width,
        renderedHeight: renderedSize.height,
        sourceWidth,
        sourceHeight,
      }),
    )
    return {
      left: `${rect.left}px`,
      top: `${rect.top}px`,
      width: `${rect.width}px`,
      height: `${rect.height}px`,
    }
  }, [renderedSize, selectedRegion, sourceHeight, sourceWidth])

  function localPoint(event: PointerEvent<HTMLDivElement>): Point {
    const image = imageRef.current
    if (!image) {
      return { x: 0, y: 0 }
    }

    const bounds = image.getBoundingClientRect()
    return {
      x: Math.max(0, Math.min(event.clientX - bounds.left, bounds.width)),
      y: Math.max(0, Math.min(event.clientY - bounds.top, bounds.height)),
    }
  }

  function publishRegion(nextRect: CssRect | null) {
    const image = imageRef.current
    if (!image || !nextRect || nextRect.width < 4 || nextRect.height < 4) {
      setSelectedRegion(null)
      onRegionChange(null)
      return
    }

    const bounds = image.getBoundingClientRect()
    setRenderedSize({ width: bounds.width, height: bounds.height })
    const nextRegion = cssRectToSourceRegion(
      nextRect,
      previewScaleFromRendered({
        renderedWidth: bounds.width,
        renderedHeight: bounds.height,
        sourceWidth,
        sourceHeight,
      }),
    )
    setSelectedRegion(nextRegion)
    onRegionChange(nextRegion)
  }

  return (
    <div
      className="preview-wrap"
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId)
        const point = localPoint(event)
        setStart(point)
        const nextRect = dragToCssRect(point, point)
        publishRegion(nextRect)
      }}
      onPointerMove={(event) => {
        if (!start) {
          return
        }
        const nextRect = dragToCssRect(start, localPoint(event))
        publishRegion(nextRect)
      }}
      onPointerUp={(event) => {
        if (!start) {
          return
        }
        const nextRect = dragToCssRect(start, localPoint(event))
        setStart(null)
        publishRegion(nextRect)
      }}
    >
      <img
        ref={imageRef}
        className="preview-image"
        src={imageUrl}
        alt="Live capture preview"
        draggable={false}
        onLoad={updateRenderedSize}
      />
      <div className="selection-dim" />
      {overlayStyle ? <div className="selection-box" style={overlayStyle} /> : null}
    </div>
  )
}

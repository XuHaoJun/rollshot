import { type PointerEvent, useEffect, useMemo, useRef, useState } from 'react'
import {
  cssRectToSourceRegion,
  dragToCssRect,
  sourceRegionToCssRect,
  type CssRect,
  type Point,
  type SourceRegion,
} from '../region/geometry'

type SelectionLayerProps = {
  sourceWidth: number
  sourceHeight: number
  selectedRegion: SourceRegion | null
  disabled?: boolean
  onSelect: (region: SourceRegion) => void
  onCancel: () => void
}

export function SelectionLayer({
  sourceWidth,
  sourceHeight,
  selectedRegion,
  disabled = false,
  onSelect,
  onCancel,
}: SelectionLayerProps) {
  const layerRef = useRef<HTMLDivElement | null>(null)
  const startRef = useRef<Point | null>(null)
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
    if (!selectedRegion) {
      return null
    }
    const bounds = layerRef.current?.getBoundingClientRect()
    return sourceRegionToCssRect(selectedRegion, {
      renderedWidth: bounds?.width ?? window.innerWidth,
      renderedHeight: bounds?.height ?? window.innerHeight,
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
      className={disabled ? 'selection-layer selection-layer-disabled' : 'selection-layer'}
      onPointerDown={(event) => {
        if (disabled) {
          return
        }
        event.currentTarget.setPointerCapture(event.pointerId)
        const point = localPoint(event)
        startRef.current = point
        setDraftRect(dragToCssRect(point, point))
        setCursorPoint(point)
      }}
      onPointerMove={(event) => {
        if (disabled) {
          return
        }
        const point = localPoint(event)
        setCursorPoint(point)
        if (startRef.current) {
          setDraftRect(dragToCssRect(startRef.current, point))
        }
      }}
      onPointerUp={(event) => {
        if (disabled) {
          return
        }
        if (!startRef.current) {
          return
        }
        const nextRect = dragToCssRect(startRef.current, localPoint(event))
        startRef.current = null
        setDraftRect(nextRect)
        if (nextRect.width < 4 || nextRect.height < 4) {
          setDraftRect(null)
          return
        }
        const bounds = event.currentTarget.getBoundingClientRect()
        onSelect(
          cssRectToSourceRegion(nextRect, {
            renderedWidth: bounds.width,
            renderedHeight: bounds.height,
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

import { describe, expect, it } from 'vitest'
import {
  clampSourceRegion,
  dragToCssRect,
  cssRectToSourceRegion,
  type CssRect,
} from './geometry'

describe('region geometry', () => {
  it('normalizes a drag in any direction', () => {
    expect(dragToCssRect({ x: 80, y: 70 }, { x: 20, y: 10 })).toEqual({
      left: 20,
      top: 10,
      width: 60,
      height: 60,
    })
  })

  it('converts CSS preview coordinates to source pixels with HiDPI scale', () => {
    const cssRect: CssRect = { left: 50, top: 25, width: 200, height: 100 }
    expect(
      cssRectToSourceRegion(cssRect, {
        renderedWidth: 500,
        renderedHeight: 250,
        sourceWidth: 1000,
        sourceHeight: 500,
      }),
    ).toEqual({ x: 100, y: 50, width: 400, height: 200 })
  })

  it('clamps source region to frame bounds', () => {
    expect(
      clampSourceRegion(
        { x: -4, y: 90, width: 40, height: 30 },
        { width: 100, height: 100 },
      ),
    ).toEqual({ x: 0, y: 90, width: 36, height: 10 })
  })
})

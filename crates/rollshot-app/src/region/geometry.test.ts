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

  it('preserves fractional CSS edges by flooring origin and ceiling far edge', () => {
    const cssRect: CssRect = { left: 10.25, top: 4.5, width: 20.25, height: 10.25 }
    expect(
      cssRectToSourceRegion(cssRect, {
        renderedWidth: 333,
        renderedHeight: 222,
        sourceWidth: 1000,
        sourceHeight: 666,
      }),
    ).toEqual({ x: 30, y: 13, width: 62, height: 32 })
  })

  it('maps a full rendered preview exactly to the full source frame', () => {
    expect(
      cssRectToSourceRegion(
        { left: 0, top: 0, width: 511.5, height: 287.75 },
        {
          renderedWidth: 511.5,
          renderedHeight: 287.75,
          sourceWidth: 2560,
          sourceHeight: 1440,
        },
      ),
    ).toEqual({ x: 0, y: 0, width: 2560, height: 1440 })
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

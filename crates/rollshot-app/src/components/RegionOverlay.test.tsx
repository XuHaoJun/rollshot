import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { RegionOverlay } from './RegionOverlay'

const reactActGlobal = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT: boolean
}
reactActGlobal.IS_REACT_ACT_ENVIRONMENT = true

type Bounds = {
  width: number
  height: number
}

function pointerEvent(type: string, x: number, y: number) {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientX: x,
    clientY: y,
  })
  Object.defineProperty(event, 'pointerId', { value: 1 })
  return event
}

describe('RegionOverlay', () => {
  let container: HTMLDivElement
  let root: Root
  let bounds: Bounds
  let rectSpy: ReturnType<typeof vi.spyOn>
  let originalSetPointerCapture: typeof HTMLDivElement.prototype.setPointerCapture | undefined

  beforeEach(() => {
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
    bounds = { width: 500, height: 250 }
    rectSpy = vi
      .spyOn(HTMLImageElement.prototype, 'getBoundingClientRect')
      .mockImplementation(() => ({
        x: 0,
        y: 0,
        left: 0,
        top: 0,
        right: bounds.width,
        bottom: bounds.height,
        width: bounds.width,
        height: bounds.height,
        toJSON: () => ({}),
      }))
    originalSetPointerCapture = HTMLDivElement.prototype.setPointerCapture
    HTMLDivElement.prototype.setPointerCapture = vi.fn()
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
    rectSpy.mockRestore()
    if (originalSetPointerCapture) {
      HTMLDivElement.prototype.setPointerCapture = originalSetPointerCapture
    } else {
      Reflect.deleteProperty(HTMLDivElement.prototype, 'setPointerCapture')
    }
  })

  it('keeps the selected source region visually aligned after resize', () => {
    const onRegionChange = vi.fn()

    act(() => {
      root.render(
        <RegionOverlay
          imageUrl="blob:preview"
          sourceWidth={1000}
          sourceHeight={500}
          onRegionChange={onRegionChange}
        />,
      )
    })

    const wrap = container.querySelector('.preview-wrap')
    expect(wrap).not.toBeNull()

    act(() => {
      wrap?.dispatchEvent(pointerEvent('pointerdown', 50, 25))
    })
    act(() => {
      wrap?.dispatchEvent(pointerEvent('pointermove', 250, 125))
    })
    act(() => {
      wrap?.dispatchEvent(pointerEvent('pointerup', 250, 125))
    })

    const box = container.querySelector<HTMLElement>('.selection-box')
    expect(box?.style.left).toBe('50px')
    expect(box?.style.top).toBe('25px')
    expect(box?.style.width).toBe('200px')
    expect(box?.style.height).toBe('100px')

    act(() => {
      bounds = { width: 250, height: 125 }
      window.dispatchEvent(new Event('resize'))
    })

    expect(box?.style.left).toBe('25px')
    expect(box?.style.top).toBe('12.5px')
    expect(box?.style.width).toBe('100px')
    expect(box?.style.height).toBe('50px')
    expect(onRegionChange).toHaveBeenLastCalledWith({
      x: 100,
      y: 50,
      width: 400,
      height: 200,
    })
  })
})

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { PreviewScale } from '../region/geometry'
import { SelectionLayer } from './SelectionLayer'

const testScale: PreviewScale = {
  scaleX: 2,
  scaleY: 2,
  sourceOriginX: 0,
  sourceOriginY: 0,
  sourceWidth: 1000,
  sourceHeight: 500,
}

const reactActGlobal = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT: boolean
}
reactActGlobal.IS_REACT_ACT_ENVIRONMENT = true

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

describe('SelectionLayer', () => {
  let container: HTMLDivElement
  let root: Root
  let rectSpy: ReturnType<typeof vi.spyOn>
  let originalSetPointerCapture: typeof HTMLDivElement.prototype.setPointerCapture | undefined

  beforeEach(() => {
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
    rectSpy = vi.spyOn(HTMLDivElement.prototype, 'getBoundingClientRect').mockImplementation(
      () =>
        ({
          x: 0,
          y: 0,
          left: 0,
          top: 0,
          right: 500,
          bottom: 250,
          width: 500,
          height: 250,
          toJSON: () => ({}),
        }) as DOMRect,
    )
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

  it('publishes a source region on drag release', () => {
    const onSelect = vi.fn()

    act(() => {
      root.render(
        <SelectionLayer
          scale={testScale}
          selectedRegion={null}
          onSelect={onSelect}
          onCancel={vi.fn()}
        />,
      )
    })

    const layer = container.querySelector('.selection-layer')
    expect(layer).not.toBeNull()

    act(() => {
      layer?.dispatchEvent(pointerEvent('pointerdown', 50, 25))
      layer?.dispatchEvent(pointerEvent('pointermove', 250, 125))
      layer?.dispatchEvent(pointerEvent('pointerup', 250, 125))
    })

    expect(onSelect).toHaveBeenLastCalledWith({
      x: 100,
      y: 50,
      width: 400,
      height: 200,
    })
    expect(container.querySelector('.selection-box')).not.toBeNull()
    expect(layer?.classList.contains('selection-layer-has-rect')).toBe(true)
  })

  it('ignores tiny selections', () => {
    const onSelect = vi.fn()

    act(() => {
      root.render(
        <SelectionLayer
          scale={testScale}
          selectedRegion={null}
          onSelect={onSelect}
          onCancel={vi.fn()}
        />,
      )
    })

    const layer = container.querySelector('.selection-layer')
    act(() => {
      layer?.dispatchEvent(pointerEvent('pointerdown', 50, 25))
      layer?.dispatchEvent(pointerEvent('pointerup', 52, 27))
    })

    expect(onSelect).not.toHaveBeenCalled()
  })

  it('does not publish selections while disabled', () => {
    const onSelect = vi.fn()

    act(() => {
      root.render(
        <SelectionLayer
          scale={testScale}
          selectedRegion={{ x: 100, y: 50, width: 400, height: 200 }}
          disabled
          onSelect={onSelect}
          onCancel={vi.fn()}
        />,
      )
    })

    const layer = container.querySelector('.selection-layer')
    act(() => {
      layer?.dispatchEvent(pointerEvent('pointerdown', 10, 10))
      layer?.dispatchEvent(pointerEvent('pointermove', 200, 100))
      layer?.dispatchEvent(pointerEvent('pointerup', 200, 100))
    })

    expect(onSelect).not.toHaveBeenCalled()
    expect(container.querySelector('.selection-box')).not.toBeNull()
  })

  it('cancels on Escape', () => {
    const onCancel = vi.fn()

    act(() => {
      root.render(
        <SelectionLayer
          scale={testScale}
          selectedRegion={null}
          onSelect={vi.fn()}
          onCancel={onCancel}
        />,
      )
    })

    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    })

    expect(onCancel).toHaveBeenCalledOnce()
  })
})

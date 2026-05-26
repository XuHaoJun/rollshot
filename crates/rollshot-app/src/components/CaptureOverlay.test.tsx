import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { SessionStatus } from '../api/capture'
import { CaptureOverlay } from './CaptureOverlay'

const api = vi.hoisted(() => ({
  confirmRegion: vi.fn(),
  getFinalPreview: vi.fn(),
  getStitchPreview: vi.fn(),
  launchOptions: vi.fn(),
  overlayExclusion: vi.fn(),
  saveImage: vi.fn(),
  setInputPassthrough: vi.fn(),
  sessionStatus: vi.fn(),
  startCapture: vi.fn(),
  startStitching: vi.fn(),
  stopCapture: vi.fn(),
  stopStitching: vi.fn(),
}))

const dialog = vi.hoisted(() => ({
  save: vi.fn(),
}))

vi.mock('../api/capture', () => api)
vi.mock('@tauri-apps/plugin-dialog', () => dialog)
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    outerPosition: () => Promise.resolve({ x: 0, y: 0 }),
    scaleFactor: () => Promise.resolve(2),
  }),
}))

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

async function flush() {
  await act(async () => {
    await Promise.resolve()
  })
}

describe('CaptureOverlay', () => {
  let container: HTMLDivElement
  let root: Root
  let rectSpy: ReturnType<typeof vi.spyOn>
  let closeSpy: ReturnType<typeof vi.spyOn>
  let originalSetPointerCapture: typeof HTMLDivElement.prototype.setPointerCapture | undefined

  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
    closeSpy = vi.spyOn(window, 'close').mockImplementation(() => undefined)
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
    Object.defineProperty(URL, 'createObjectURL', {
      configurable: true,
      value: vi.fn(() => 'blob:preview'),
    })
    Object.defineProperty(URL, 'revokeObjectURL', {
      configurable: true,
      value: vi.fn(),
    })
    api.launchOptions.mockResolvedValue({
      backend: 'fixture',
      fps: 5,
      show_cursor: false,
    })
    api.overlayExclusion.mockResolvedValue('unsupported')
    api.startCapture.mockResolvedValue(undefined)
    api.setInputPassthrough.mockResolvedValue(undefined)
    api.sessionStatus.mockResolvedValue({
      state: 'previewing',
      frame_width: 1000,
      frame_height: 500,
      region: null,
    } satisfies SessionStatus)
    api.confirmRegion.mockResolvedValue({ x: 100, y: 50, width: 400, height: 200 })
    api.startStitching.mockResolvedValue(undefined)
    api.getStitchPreview.mockResolvedValue(null)
    api.stopStitching.mockResolvedValue({ image_width: 1000, image_height: 1600 })
    api.getFinalPreview.mockResolvedValue(new Blob(['png'], { type: 'image/png' }))
    api.saveImage.mockResolvedValue({ image_width: 1000, image_height: 1600, output_path: '/tmp/rollshot.png' })
    dialog.save.mockResolvedValue(null)
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
    closeSpy.mockRestore()
    rectSpy.mockRestore()
    if (originalSetPointerCapture) {
      HTMLDivElement.prototype.setPointerCapture = originalSetPointerCapture
    } else {
      Reflect.deleteProperty(HTMLDivElement.prototype, 'setPointerCapture')
    }
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('starts capture from launch options when mounted', async () => {
    act(() => root.render(<CaptureOverlay />))
    await flush()

    expect(api.launchOptions).toHaveBeenCalledOnce()
    expect(api.overlayExclusion).toHaveBeenCalledOnce()
    expect(api.startCapture).toHaveBeenCalledWith({
      backend: 'fixture',
      fps: 5,
      show_cursor: false,
    })
  })

  it('hides the picker and waits one frame before starting stitching', async () => {
    act(() => root.render(<CaptureOverlay />))
    await flush()
    await act(async () => {
      await vi.advanceTimersByTimeAsync(160)
    })

    const layer = container.querySelector('.selection-layer')
    expect(layer).not.toBeNull()

    await act(async () => {
      layer?.dispatchEvent(pointerEvent('pointerdown', 50, 25))
      layer?.dispatchEvent(pointerEvent('pointermove', 250, 125))
      layer?.dispatchEvent(pointerEvent('pointerup', 250, 125))
      await Promise.resolve()
    })

    expect(api.confirmRegion).toHaveBeenCalledWith({ x: 100, y: 50, width: 400, height: 200 })
    expect(container.querySelector('.selection-box')).toBeNull()
    expect(container.querySelector('.capture-mask')).not.toBeNull()
    expect(api.startStitching).not.toHaveBeenCalled()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(17)
    })

    expect(api.startStitching).toHaveBeenCalledOnce()
  })

  it('stops stitching and requests the final preview', async () => {
    api.sessionStatus.mockResolvedValue({
      state: 'stitching',
      frame_width: 1000,
      frame_height: 500,
      region: { x: 100, y: 50, width: 400, height: 200 },
      stats: { frame_count: 3, total_width: 400, total_height: 900, last_append: 200 },
      last_outcome: 'appended',
    } satisfies SessionStatus)

    act(() => root.render(<CaptureOverlay />))
    await flush()
    await act(async () => {
      await vi.advanceTimersByTimeAsync(160)
    })

    const stopButton = Array.from(container.querySelectorAll('button')).find(
      (button) => button.textContent?.includes('Stop'),
    )
    expect(stopButton).not.toBeUndefined()

    await act(async () => {
      stopButton?.dispatchEvent(new MouseEvent('click', { bubbles: true }))
      await Promise.resolve()
    })

    expect(api.stopStitching).toHaveBeenCalledOnce()
    expect(api.getFinalPreview).toHaveBeenCalledWith(1400)
  })

  it('enables input passthrough while stitching and disables it on unmount', async () => {
    api.sessionStatus.mockResolvedValue({
      state: 'stitching',
      frame_width: 1000,
      frame_height: 500,
      region: { x: 100, y: 50, width: 400, height: 200 },
      stats: { frame_count: 3, total_width: 400, total_height: 900, last_append: 200 },
      last_outcome: 'appended',
    } satisfies SessionStatus)

    act(() => root.render(<CaptureOverlay />))
    await flush()
    await act(async () => {
      await vi.advanceTimersByTimeAsync(160)
    })

    expect(api.setInputPassthrough).toHaveBeenCalledWith(true)

    await act(async () => root.unmount())

    expect(api.setInputPassthrough).toHaveBeenLastCalledWith(false)
  })

  it('renders a transparent crop mask without picker chrome while stitching', async () => {
    api.sessionStatus.mockResolvedValue({
      state: 'stitching',
      frame_width: 1000,
      frame_height: 500,
      region: { x: 100, y: 50, width: 400, height: 200 },
      stats: { frame_count: 3, total_width: 400, total_height: 900, last_append: 200 },
      last_outcome: 'appended',
    } satisfies SessionStatus)

    act(() => root.render(<CaptureOverlay />))
    await flush()
    await act(async () => {
      await vi.advanceTimersByTimeAsync(160)
    })

    expect(container.querySelector('.selection-box')).toBeNull()
    expect(container.querySelector('.capture-mask')).not.toBeNull()
  })

  it('finishes stitching and opens the save dialog on Escape', async () => {
    api.sessionStatus.mockResolvedValue({
      state: 'stitching',
      frame_width: 1000,
      frame_height: 500,
      region: { x: 100, y: 50, width: 400, height: 200 },
      stats: { frame_count: 3, total_width: 400, total_height: 900, last_append: 200 },
      last_outcome: 'appended',
    } satisfies SessionStatus)
    dialog.save.mockResolvedValue('/tmp/rollshot.png')

    act(() => root.render(<CaptureOverlay />))
    await flush()
    await act(async () => {
      await vi.advanceTimersByTimeAsync(160)
    })

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
      await Promise.resolve()
    })

    expect(api.stopStitching).toHaveBeenCalledOnce()
    expect(dialog.save).toHaveBeenCalledWith({
      title: 'Save stitched PNG',
      defaultPath: 'rollshot.png',
      filters: [{ name: 'PNG image', extensions: ['png'] }],
    })
    expect(api.saveImage).toHaveBeenCalledWith('/tmp/rollshot.png')
    expect(api.stopCapture).toHaveBeenCalledOnce()
    expect(closeSpy).toHaveBeenCalledOnce()
  })

  it('closes after Escape when the save dialog is cancelled', async () => {
    api.sessionStatus.mockResolvedValue({
      state: 'stitching',
      frame_width: 1000,
      frame_height: 500,
      region: { x: 100, y: 50, width: 400, height: 200 },
      stats: { frame_count: 3, total_width: 400, total_height: 900, last_append: 200 },
      last_outcome: 'appended',
    } satisfies SessionStatus)
    dialog.save.mockResolvedValue(null)

    act(() => root.render(<CaptureOverlay />))
    await flush()
    await act(async () => {
      await vi.advanceTimersByTimeAsync(160)
    })

    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
      await Promise.resolve()
    })

    expect(api.stopStitching).toHaveBeenCalledOnce()
    expect(api.saveImage).not.toHaveBeenCalled()
    expect(api.stopCapture).toHaveBeenCalledOnce()
    expect(closeSpy).toHaveBeenCalledOnce()
  })
})

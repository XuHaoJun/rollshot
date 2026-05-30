import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

describe('capture api wrappers', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('sends start_stitching without payload', async () => {
    const { startStitching } = await import('./capture')
    invokeMock.mockResolvedValueOnce(undefined)

    await startStitching()

    expect(invokeMock).toHaveBeenCalledWith('start_stitching')
  })

  it('saves final image to selected path', async () => {
    const { saveImage } = await import('./capture')
    invokeMock.mockResolvedValueOnce({
      image_width: 100,
      image_height: 400,
      output_path: '/tmp/out.png',
    })

    await expect(saveImage('/tmp/out.png')).resolves.toEqual({
      image_width: 100,
      image_height: 400,
      output_path: '/tmp/out.png',
    })
    expect(invokeMock).toHaveBeenCalledWith('save_image', { path: '/tmp/out.png' })
  })

  it('runs native capture and returns the done image dto', async () => {
    const { runNativeCapture } = await import('./capture')
    invokeMock.mockResolvedValueOnce({
      image_width: 800,
      image_height: 1200,
      output_path: null,
    })

    await expect(
      runNativeCapture({ backend: 'auto', fps: 30, show_cursor: false }),
    ).resolves.toEqual({ image_width: 800, image_height: 1200, output_path: null })
    expect(invokeMock).toHaveBeenCalledWith('run_native_capture', {
      options: { backend: 'auto', fps: 30, show_cursor: false },
    })
  })

  it('returns null when native capture is cancelled', async () => {
    const { runNativeCapture } = await import('./capture')
    invokeMock.mockResolvedValueOnce(null)

    await expect(
      runNativeCapture({ backend: 'auto', fps: 30, show_cursor: false }),
    ).resolves.toBeNull()
  })

  it('reads the native overlay capability flag', async () => {
    const { usesNativeOverlay } = await import('./capture')
    invokeMock.mockResolvedValueOnce(true)

    await expect(usesNativeOverlay()).resolves.toBe(true)
    expect(invokeMock).toHaveBeenCalledWith('uses_native_overlay')
  })

  it('sends stop_stitching and returns done image dto', async () => {
    const { stopStitching } = await import('./capture')
    invokeMock.mockResolvedValueOnce({
      image_width: 200,
      image_height: 600,
      output_path: null,
    })

    await expect(stopStitching()).resolves.toEqual({
      image_width: 200,
      image_height: 600,
      output_path: null,
    })
    expect(invokeMock).toHaveBeenCalledWith('stop_stitching')
  })

  it('returns null when final preview is not available yet', async () => {
    const { getFinalPreview } = await import('./capture')
    invokeMock.mockResolvedValueOnce(new ArrayBuffer(0))

    await expect(getFinalPreview(1200)).resolves.toBeNull()
  })

  it('reads overlay exclusion capability', async () => {
    const { overlayExclusion } = await import('./capture')
    invokeMock.mockResolvedValueOnce('unsupported')

    await expect(overlayExclusion()).resolves.toBe('unsupported')
    expect(invokeMock).toHaveBeenCalledWith('overlay_exclusion')
  })

  it('sets native input passthrough', async () => {
    const { setInputPassthrough } = await import('./capture')
    invokeMock.mockResolvedValueOnce(undefined)

    await setInputPassthrough(true)

    expect(invokeMock).toHaveBeenCalledWith('set_input_passthrough', { enabled: true })
  })
})

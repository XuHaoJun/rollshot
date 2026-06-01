import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { NativeCaptureFlow } from './NativeCaptureFlow'

const api = vi.hoisted(() => ({
  exitApp: vi.fn(),
  launchOptions: vi.fn(),
  runNativeCapture: vi.fn(),
}))
const saveApi = vi.hoisted(() => ({
  promptSaveStitchedPng: vi.fn(),
}))
const dialog = vi.hoisted(() => ({
  message: vi.fn(),
}))

vi.mock('../api/capture', () => api)
vi.mock('../api/save', () => saveApi)
vi.mock('@tauri-apps/plugin-dialog', () => dialog)

const reactActGlobal = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT: boolean
}
reactActGlobal.IS_REACT_ACT_ENVIRONMENT = true

async function flush() {
  for (let i = 0; i < 6; i += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

describe('NativeCaptureFlow', () => {
  let container: HTMLDivElement
  let root: Root

  beforeEach(() => {
    vi.clearAllMocks()
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
    api.launchOptions.mockResolvedValue({ backend: 'auto', fps: 30, show_cursor: false })
    saveApi.promptSaveStitchedPng.mockResolvedValue(true)
    dialog.message.mockResolvedValue(undefined)
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
  })

  it('opens the save flow and closes the window when capture finishes', async () => {
    api.runNativeCapture.mockResolvedValue({
      image_width: 800,
      image_height: 1200,
      output_path: null,
    })

    await act(async () => {
      root.render(<NativeCaptureFlow />)
    })
    await flush()

    expect(api.runNativeCapture).toHaveBeenCalledWith({
      backend: 'auto',
      fps: 30,
      show_cursor: false,
    })
    expect(saveApi.promptSaveStitchedPng).toHaveBeenCalledTimes(1)
    expect(api.exitApp).toHaveBeenCalledTimes(1)
  })

  it('shows a save error and exits without retrying', async () => {
    api.runNativeCapture.mockResolvedValue({
      image_width: 800,
      image_height: 1200,
      output_path: null,
    })
    saveApi.promptSaveStitchedPng
      .mockRejectedValueOnce(new Error('disk full'))

    await act(async () => {
      root.render(<NativeCaptureFlow />)
    })
    await flush()

    expect(dialog.message).toHaveBeenCalledWith('Error: disk full', {
      title: 'Rollshot save failed',
      kind: 'error',
    })
    expect(saveApi.promptSaveStitchedPng).toHaveBeenCalledTimes(1)
    expect(api.exitApp).toHaveBeenCalledTimes(1)
  })

  it('closes without saving when capture is cancelled', async () => {
    api.runNativeCapture.mockResolvedValue(null)

    await act(async () => {
      root.render(<NativeCaptureFlow />)
    })
    await flush()

    expect(saveApi.promptSaveStitchedPng).not.toHaveBeenCalled()
    expect(api.exitApp).toHaveBeenCalledTimes(1)
  })

  it('shows an error dialog and closes the window when capture fails', async () => {
    api.runNativeCapture.mockRejectedValue(new Error('portal denied'))

    await act(async () => {
      root.render(<NativeCaptureFlow />)
    })
    await flush()

    expect(saveApi.promptSaveStitchedPng).not.toHaveBeenCalled()
    expect(dialog.message).toHaveBeenCalledWith('Error: portal denied', {
      title: 'Rollshot capture failed',
      kind: 'error',
    })
    expect(api.exitApp).toHaveBeenCalledTimes(1)
  })
})

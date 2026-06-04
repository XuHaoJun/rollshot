import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'

const api = vi.hoisted(() => ({
  usesNativeOverlay: vi.fn(),
}))
const renders = vi.hoisted(() => ({
  native: vi.fn(),
  webview: vi.fn(),
}))

vi.mock('./api/capture', () => api)
vi.mock('./components/NativeCaptureFlow', () => ({
  NativeCaptureFlow: () => {
    renders.native()
    return null
  },
}))
vi.mock('./components/CaptureOverlay', () => ({
  CaptureOverlay: () => {
    renders.webview()
    return null
  },
}))

const reactActGlobal = globalThis as typeof globalThis & {
  IS_REACT_ACT_ENVIRONMENT: boolean
}
reactActGlobal.IS_REACT_ACT_ENVIRONMENT = true

async function flush() {
  for (let i = 0; i < 4; i += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

describe('App', () => {
  let container: HTMLDivElement
  let root: Root

  beforeEach(() => {
    vi.clearAllMocks()
    container = document.createElement('div')
    document.body.append(container)
    root = createRoot(container)
  })

  afterEach(() => {
    act(() => root.unmount())
    container.remove()
  })

  it('renders NativeCaptureFlow when the backend uses the native overlay', async () => {
    api.usesNativeOverlay.mockResolvedValue(true)

    await act(async () => {
      root.render(<App />)
    })
    await flush()

    expect(renders.native).toHaveBeenCalled()
    expect(renders.webview).not.toHaveBeenCalled()
  })

  it('renders CaptureOverlay when the backend uses the webview overlay', async () => {
    api.usesNativeOverlay.mockResolvedValue(false)

    await act(async () => {
      root.render(<App />)
    })
    await flush()

    expect(renders.webview).toHaveBeenCalled()
    expect(renders.native).not.toHaveBeenCalled()
  })

  it('falls back to the webview overlay when the capability query fails', async () => {
    api.usesNativeOverlay.mockRejectedValue(new Error('ipc down'))

    await act(async () => {
      root.render(<App />)
    })
    await flush()

    expect(renders.webview).toHaveBeenCalled()
    expect(renders.native).not.toHaveBeenCalled()
  })
})

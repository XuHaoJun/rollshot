import { beforeEach, describe, expect, it, vi } from 'vitest'

const dialog = vi.hoisted(() => ({
  save: vi.fn(),
}))
const capture = vi.hoisted(() => ({
  saveImage: vi.fn(),
}))

vi.mock('@tauri-apps/plugin-dialog', () => dialog)
vi.mock('./capture', () => capture)

describe('promptSaveStitchedPng', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('writes the selected path and reports the saved output', async () => {
    const { promptSaveStitchedPng } = await import('./save')
    const onMessage = vi.fn()
    dialog.save.mockResolvedValueOnce('/tmp/rollshot.png')
    capture.saveImage.mockResolvedValueOnce({
      image_width: 100,
      image_height: 400,
      output_path: '/tmp/rollshot.png',
    })

    await promptSaveStitchedPng(onMessage)

    expect(dialog.save).toHaveBeenCalledWith({
      title: 'Save stitched PNG',
      defaultPath: 'rollshot.png',
      filters: [{ name: 'PNG image', extensions: ['png'] }],
    })
    expect(capture.saveImage).toHaveBeenCalledWith('/tmp/rollshot.png')
    expect(onMessage).toHaveBeenCalledWith('Saved /tmp/rollshot.png')
  })

  it('does not write when the save dialog is cancelled', async () => {
    const { promptSaveStitchedPng } = await import('./save')
    const onMessage = vi.fn()
    dialog.save.mockResolvedValueOnce(null)

    await promptSaveStitchedPng(onMessage)

    expect(capture.saveImage).not.toHaveBeenCalled()
    expect(onMessage).not.toHaveBeenCalled()
  })

  it('propagates save failures', async () => {
    const { promptSaveStitchedPng } = await import('./save')
    dialog.save.mockResolvedValueOnce('/tmp/rollshot.png')
    capture.saveImage.mockRejectedValueOnce(new Error('disk full'))

    await expect(promptSaveStitchedPng()).rejects.toThrow('disk full')
  })
})

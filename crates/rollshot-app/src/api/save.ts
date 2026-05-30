import { save } from '@tauri-apps/plugin-dialog'
import { saveImage } from './capture'

export async function promptSaveStitchedPng(
  onMessage?: (message: string) => void,
): Promise<void> {
  const selected = await save({
    title: 'Save stitched PNG',
    defaultPath: 'rollshot.png',
    filters: [{ name: 'PNG image', extensions: ['png'] }],
  })
  if (selected) {
    const done = await saveImage(selected)
    onMessage?.(done.output_path ? `Saved ${done.output_path}` : 'Saved image')
  }
}

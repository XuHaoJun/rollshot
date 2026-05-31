import { useEffect, useRef, useState } from 'react'
import { message as showMessage } from '@tauri-apps/plugin-dialog'
import { exitApp, launchOptions, runNativeCapture } from '../api/capture'
import { promptSaveStitchedPng } from '../api/save'

export function NativeCaptureFlow() {
  const [message, setMessage] = useState('Starting capture')
  const startedRef = useRef(false)

  useEffect(() => {
    if (startedRef.current) {
      return
    }
    startedRef.current = true

    void (async () => {
      try {
        const options = await launchOptions()
        const done = await runNativeCapture(options)
        if (done) {
          setMessage(`Stitched ${done.image_width}x${done.image_height}`)
          let savedOrCancelled = false
          while (!savedOrCancelled) {
            try {
              savedOrCancelled = await promptSaveStitchedPng(setMessage)
            } catch (saveError) {
              const saveErrorMessage = String(saveError)
              setMessage(saveErrorMessage)
              try {
                await showMessage(saveErrorMessage, {
                  title: 'Rollshot save failed',
                  kind: 'error',
                })
              } catch {
                // Keep the finished image in session and retry the save prompt.
              }
            }
          }
        }
      } catch (error) {
        const errorMessage = String(error)
        setMessage(errorMessage)
        try {
          await showMessage(errorMessage, {
            title: 'Rollshot capture failed',
            kind: 'error',
          })
        } catch {
          // The native capture already failed; still close the hidden host
          // window if the best-effort error dialog cannot be shown.
        }
      } finally {
        await exitApp()
      }
    })()
  }, [])

  return (
    <main className="capture-overlay">
      <div className="capture-status">{message}</div>
    </main>
  )
}

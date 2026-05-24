import { Check, Play, Save, Square, Wand2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import {
  confirmRegion,
  getFinalPreview,
  getLatestPreview,
  launchOptions,
  saveImage,
  sessionStatus,
  startCapture,
  startStitching,
  stopCapture,
  stopStitching,
  type InteractiveLaunchOptions,
  type SessionStatus,
} from './api/capture'
import { save } from '@tauri-apps/plugin-dialog'
import { Button } from '@/components/ui/button'
import { RegionOverlay } from './components/RegionOverlay'
import type { SourceRegion } from './region/geometry'

export default function App() {
  const [status, setStatus] = useState<SessionStatus>({ state: 'idle' })
  const [options, setOptions] = useState<InteractiveLaunchOptions | null>(null)
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const [finalPreviewUrl, setFinalPreviewUrl] = useState<string | null>(null)
  const [pendingRegion, setPendingRegion] = useState<SourceRegion | null>(null)
  const [message, setMessage] = useState('Ready to start capture')
  const previewUrlRef = useRef<string | null>(null)
  const finalPreviewUrlRef = useRef<string | null>(null)
  const previewPollInFlightRef = useRef(false)

  useEffect(() => {
    previewUrlRef.current = previewUrl
  }, [previewUrl])

  useEffect(() => {
    finalPreviewUrlRef.current = finalPreviewUrl
  }, [finalPreviewUrl])

  useEffect(() => {
    launchOptions()
      .then(setOptions)
      .catch((error) => setMessage(String(error)))
  }, [])

  useEffect(() => {
    return () => {
      if (previewUrlRef.current) {
        URL.revokeObjectURL(previewUrlRef.current)
      }
      if (finalPreviewUrlRef.current) {
        URL.revokeObjectURL(finalPreviewUrlRef.current)
      }
    }
  }, [])

  useEffect(() => {
    const timer = window.setInterval(async () => {
      if (previewPollInFlightRef.current) {
        return
      }

      previewPollInFlightRef.current = true
      try {
        const nextStatus = await sessionStatus()
        setStatus(nextStatus)

        if (nextStatus.state === 'previewing' || nextStatus.state === 'stitching') {
          const blob = await getLatestPreview(1400)
          if (blob) {
            const nextUrl = URL.createObjectURL(blob)
            setPreviewUrl((oldUrl) => {
              if (oldUrl) {
                URL.revokeObjectURL(oldUrl)
              }
              return nextUrl
            })
          }
        }
      } catch (error) {
        setMessage(String(error))
      } finally {
        previewPollInFlightRef.current = false
      }
    }, 160)

    return () => window.clearInterval(timer)
  }, [])

  async function onStart() {
    if (!options) {
      setMessage('Launch options are not loaded yet')
      return
    }
    try {
      setMessage('Starting capture')
      await startCapture(options)
      setMessage('Select a region in the preview')
    } catch (error) {
      setMessage(String(error))
    }
  }

  async function onConfirmRegion() {
    if (!pendingRegion) {
      setMessage('Select a region first')
      return
    }
    try {
      const confirmed = await confirmRegion(pendingRegion)
      setMessage(
        `Region ${confirmed.width}x${confirmed.height} at ${confirmed.x},${confirmed.y}`,
      )
    } catch (error) {
      setMessage(String(error))
    }
  }

  async function onStartStitching() {
    try {
      setMessage('Stitching started. Scroll the selected content, then stop.')
      await startStitching()
    } catch (error) {
      setMessage(String(error))
    }
  }

  async function refreshFinalPreview() {
    const blob = await getFinalPreview(1400)
    if (!blob) {
      return
    }
    const nextUrl = URL.createObjectURL(blob)
    setFinalPreviewUrl((oldUrl) => {
      if (oldUrl) {
        URL.revokeObjectURL(oldUrl)
      }
      return nextUrl
    })
  }

  async function onStop() {
    try {
      if (status.state === 'stitching') {
        const done = await stopStitching()
        setMessage(`Stitched ${done.image_width}x${done.image_height}`)
        try {
          await refreshFinalPreview()
        } catch {
          // refresh failure is non-fatal; keep the success message
        }
        return
      }

      await stopCapture()
      setMessage('Capture stopped')
    } catch (error) {
      setMessage(String(error))
    }
  }

  async function onSave() {
    try {
      const selected = await save({
        title: 'Save stitched PNG',
        defaultPath: 'rollshot.png',
        filters: [{ name: 'PNG image', extensions: ['png'] }],
      })
      if (!selected) {
        return
      }

      const done = await saveImage(selected)
      setMessage(done.output_path ? `Saved ${done.output_path}` : 'Saved image')
    } catch (error) {
      setMessage(String(error))
    }
  }

  const canConfirm =
    status.state === 'previewing' &&
    pendingRegion !== null &&
    pendingRegion.width > 0 &&
    pendingRegion.height > 0
  const canStartStitching = status.state === 'previewing' && status.region !== null
  const canSave = status.state === 'done'
  const statsText =
    status.state === 'stitching'
      ? `${status.stats.frame_count} frames, ${status.stats.total_width}x${status.stats.total_height}`
      : null

  return (
    <main className="app-shell">
      <section className="capture-surface">
        {status.state === 'done' && finalPreviewUrl ? (
          <img className="final-preview-image" src={finalPreviewUrl} alt="Stitched result" />
        ) : status.state === 'previewing' && previewUrl ? (
          <RegionOverlay
            imageUrl={previewUrl}
            sourceWidth={status.frame_width}
            sourceHeight={status.frame_height}
            onRegionChange={setPendingRegion}
          />
        ) : status.state === 'stitching' && previewUrl ? (
          <img className="preview-image" src={previewUrl} alt="Live capture preview" />
        ) : (
          <div className="empty-preview">No preview yet</div>
        )}
      </section>
      <aside className="control-panel" aria-label="Capture controls">
        <h1>rollshot</h1>
        <p className="status-text">
          {status.state === 'failed' ? status.message : message}
        </p>
        <Button type="button" onClick={onStart} disabled={status.state === 'stitching'}>
          <Play className="size-4" aria-hidden="true" />
          Start
        </Button>
        <Button
          type="button"
          variant="secondary"
          disabled={!canConfirm}
          onClick={onConfirmRegion}
        >
          <Check className="size-4" aria-hidden="true" />
          Confirm Region
        </Button>
        <Button
          type="button"
          variant="secondary"
          disabled={!canStartStitching}
          onClick={onStartStitching}
        >
          <Wand2 className="size-4" aria-hidden="true" />
          Start Stitching
        </Button>
        <Button type="button" variant="outline" onClick={onStop}>
          <Square className="size-4" aria-hidden="true" />
          Stop
        </Button>
        <Button type="button" disabled={!canSave} onClick={onSave}>
          <Save className="size-4" aria-hidden="true" />
          Save
        </Button>
        {statsText ? <p className="stats-text">{statsText}</p> : null}
        {status.state === 'stitching' && status.last_outcome ? (
          <p className="stats-text">{status.last_outcome}</p>
        ) : null}
      </aside>
    </main>
  )
}

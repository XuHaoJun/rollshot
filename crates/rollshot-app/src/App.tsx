import { Check, Play, Square } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import {
  confirmRegion,
  getLatestPreview,
  launchOptions,
  sessionStatus,
  startCapture,
  stopCapture,
  type InteractiveLaunchOptions,
  type SessionStatus,
} from './api/capture'
import { Button } from '@/components/ui/button'
import { RegionOverlay } from './components/RegionOverlay'
import type { SourceRegion } from './region/geometry'

export default function App() {
  const [status, setStatus] = useState<SessionStatus>({ state: 'idle' })
  const [options, setOptions] = useState<InteractiveLaunchOptions | null>(null)
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const [pendingRegion, setPendingRegion] = useState<SourceRegion | null>(null)
  const [message, setMessage] = useState('Ready to start capture')
  const previewUrlRef = useRef<string | null>(null)

  useEffect(() => {
    previewUrlRef.current = previewUrl
  }, [previewUrl])

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
    }
  }, [])

  useEffect(() => {
    const timer = window.setInterval(async () => {
      try {
        const nextStatus = await sessionStatus()
        setStatus(nextStatus)

        if (nextStatus.state === 'previewing') {
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

  async function onStop() {
    try {
      await stopCapture()
      setMessage('Capture stopped')
    } catch (error) {
      setMessage(String(error))
    }
  }

  const canConfirm =
    status.state === 'previewing' &&
    pendingRegion !== null &&
    pendingRegion.width > 0 &&
    pendingRegion.height > 0

  return (
    <main className="app-shell">
      <section className="capture-surface">
        {status.state === 'previewing' && previewUrl ? (
          <RegionOverlay
            imageUrl={previewUrl}
            sourceWidth={status.frame_width}
            sourceHeight={status.frame_height}
            onRegionChange={setPendingRegion}
          />
        ) : (
          <div className="empty-preview">No preview yet</div>
        )}
      </section>
      <aside className="control-panel" aria-label="Capture controls">
        <h1>rollshot</h1>
        <p className="status-text">
          {status.state === 'failed' ? status.message : message}
        </p>
        <Button type="button" onClick={onStart}>
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
        <Button type="button" variant="outline" onClick={onStop}>
          <Square className="size-4" aria-hidden="true" />
          Stop
        </Button>
      </aside>
    </main>
  )
}

import { Play, Save, Square } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import {
  confirmRegion,
  getFinalPreview,
  getLatestPreview,
  getStitchPreview,
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
  const [stitchPreviewUrl, setStitchPreviewUrl] = useState<string | null>(null)
  const [finalPreviewUrl, setFinalPreviewUrl] = useState<string | null>(null)
  const [pendingRegion, setPendingRegion] = useState<SourceRegion | null>(null)
  const [message, setMessage] = useState('Ready to start capture')
  const previewUrlRef = useRef<string | null>(null)
  const stitchPreviewUrlRef = useRef<string | null>(null)
  const finalPreviewUrlRef = useRef<string | null>(null)
  const previewPollInFlightRef = useRef(false)

  useEffect(() => { previewUrlRef.current = previewUrl }, [previewUrl])
  useEffect(() => { stitchPreviewUrlRef.current = stitchPreviewUrl }, [stitchPreviewUrl])
  useEffect(() => { finalPreviewUrlRef.current = finalPreviewUrl }, [finalPreviewUrl])

  useEffect(() => {
    launchOptions()
      .then(setOptions)
      .catch((error) => setMessage(String(error)))
  }, [])

  useEffect(() => {
    return () => {
      if (previewUrlRef.current) URL.revokeObjectURL(previewUrlRef.current)
      if (stitchPreviewUrlRef.current) URL.revokeObjectURL(stitchPreviewUrlRef.current)
      if (finalPreviewUrlRef.current) URL.revokeObjectURL(finalPreviewUrlRef.current)
    }
  }, [])

  useEffect(() => {
    const timer = window.setInterval(async () => {
      if (previewPollInFlightRef.current) return
      previewPollInFlightRef.current = true
      try {
        const nextStatus = await sessionStatus()
        setStatus(nextStatus)

        if (nextStatus.state === 'previewing' || nextStatus.state === 'stitching') {
          const blob = await getLatestPreview(1400)
          if (blob) {
            const nextUrl = URL.createObjectURL(blob)
            setPreviewUrl((oldUrl) => {
              if (oldUrl) URL.revokeObjectURL(oldUrl)
              return nextUrl
            })
          }
        }

        if (nextStatus.state === 'stitching') {
          const blob = await getStitchPreview(600)
          if (blob) {
            const nextUrl = URL.createObjectURL(blob)
            setStitchPreviewUrl((oldUrl) => {
              if (oldUrl) URL.revokeObjectURL(oldUrl)
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
    setPendingRegion(null)
    setStitchPreviewUrl((old) => { if (old) URL.revokeObjectURL(old); return null })
    try {
      setMessage('Starting capture…')
      await startCapture(options)
      setMessage('Draw a region to capture')
    } catch (error) {
      setMessage(String(error))
    }
  }

  async function onConfirmRegion() {
    if (!pendingRegion) {
      setMessage('Draw a region first')
      return
    }
    try {
      const confirmed = await confirmRegion(pendingRegion)
      setMessage(`Region ${confirmed.width}×${confirmed.height} — scroll now`)
      await startStitching()
    } catch (error) {
      setMessage(String(error))
    }
  }

  async function onStop() {
    try {
      if (status.state === 'stitching') {
        const done = await stopStitching()
        setMessage(`Stitched ${done.image_width}×${done.image_height}`)
        const blob = await getFinalPreview(1400)
        if (blob) {
          const nextUrl = URL.createObjectURL(blob)
          setFinalPreviewUrl((old) => { if (old) URL.revokeObjectURL(old); return nextUrl })
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
      if (!selected) return
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

  const isStitching = status.state === 'stitching'
  const isDone = status.state === 'done'
  const statsText =
    isStitching
      ? `${status.stats.frame_count} frames · ${status.stats.total_width}×${status.stats.total_height}px`
      : null

  return (
    <main className="app-shell">
      {/* ── Left: live capture surface ── */}
      <section className="capture-surface">
        {isDone && finalPreviewUrl ? (
          <img className="final-preview-image" src={finalPreviewUrl} alt="Stitched result" />
        ) : (status.state === 'previewing' || isStitching) && previewUrl ? (
          status.state === 'previewing' ? (
            <RegionOverlay
              imageUrl={previewUrl}
              sourceWidth={status.frame_width}
              sourceHeight={status.frame_height}
              onRegionChange={setPendingRegion}
            />
          ) : (
            <img className="preview-image" src={previewUrl} alt="Live capture" />
          )
        ) : (
          <div className="empty-preview">No preview yet</div>
        )}
      </section>

      {/* ── Right: stitch preview (stitching) or controls ── */}
      {isStitching ? (
        <aside className="stitch-panel">
          <div className="stitch-preview-scroll">
            {stitchPreviewUrl ? (
              <img className="stitch-preview-image" src={stitchPreviewUrl} alt="Stitching preview" />
            ) : (
              <div className="stitch-preview-empty">Stitching…</div>
            )}
          </div>
          <div className="stitch-panel-footer">
            {statsText && <p className="stats-text">{statsText}</p>}
            {status.last_outcome && <p className="stats-text">{status.last_outcome}</p>}
            <Button type="button" variant="outline" onClick={onStop}>
              <Square className="size-4" aria-hidden="true" />
              Stop
            </Button>
          </div>
        </aside>
      ) : (
        <aside className="control-panel" aria-label="Capture controls">
          <h1>rollshot</h1>
          <p className="status-text">
            {status.state === 'failed' ? status.message : message}
          </p>
          <Button type="button" onClick={onStart} disabled={isStitching}>
            <Play className="size-4" aria-hidden="true" />
            Start
          </Button>
          {status.state === 'previewing' && (
            <Button
              type="button"
              variant="secondary"
              disabled={!canConfirm}
              onClick={onConfirmRegion}
            >
              Capture Region
            </Button>
          )}
          {isDone && (
            <>
              <Button type="button" variant="outline" onClick={onStop}>
                <Square className="size-4" aria-hidden="true" />
                Stop
              </Button>
              <Button type="button" onClick={onSave}>
                <Save className="size-4" aria-hidden="true" />
                Save
              </Button>
            </>
          )}
          {!isDone && status.state !== 'idle' && status.state !== 'previewing' && (
            <Button type="button" variant="outline" onClick={onStop}>
              <Square className="size-4" aria-hidden="true" />
              Stop
            </Button>
          )}
        </aside>
      )}
    </main>
  )
}

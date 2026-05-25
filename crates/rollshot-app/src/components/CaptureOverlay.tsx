import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { save } from '@tauri-apps/plugin-dialog'
import {
  confirmRegion,
  getFinalPreview,
  getStitchPreview,
  launchOptions,
  overlayExclusion,
  saveImage,
  sessionStatus,
  startCapture,
  startStitching,
  stopCapture,
  stopStitching,
  type OverlayExclusion,
  type SessionStatus,
} from '../api/capture'
import type { SourceRegion } from '../region/geometry'
import { sourceRegionToCssRect } from '../region/geometry'
import { choosePreviewPlacement } from '../overlay/placement'
import { AdaptiveStitchPreview } from './AdaptiveStitchPreview'
import { OverlayToolbar } from './OverlayToolbar'
import { SelectionLayer } from './SelectionLayer'

const PREVIEW_SIZE = { width: 180, height: 260 }

export function CaptureOverlay() {
  const [status, setStatus] = useState<SessionStatus>({ state: 'idle' })
  const [overlayMode, setOverlayMode] = useState<OverlayExclusion>('unknown')
  const [selectedRegion, setSelectedRegion] = useState<SourceRegion | null>(null)
  const [stitchPreviewUrl, setStitchPreviewUrl] = useState<string | null>(null)
  const [finalPreviewUrl, setFinalPreviewUrl] = useState<string | null>(null)
  const [message, setMessage] = useState('Starting capture')
  const [startupFailed, setStartupFailed] = useState(false)
  const pollInFlightRef = useRef(false)
  const stitchPreviewUrlRef = useRef<string | null>(null)
  const finalPreviewUrlRef = useRef<string | null>(null)

  useEffect(() => {
    stitchPreviewUrlRef.current = stitchPreviewUrl
  }, [stitchPreviewUrl])

  useEffect(() => {
    finalPreviewUrlRef.current = finalPreviewUrl
  }, [finalPreviewUrl])

  useEffect(() => {
    return () => {
      if (stitchPreviewUrlRef.current) URL.revokeObjectURL(stitchPreviewUrlRef.current)
      if (finalPreviewUrlRef.current) URL.revokeObjectURL(finalPreviewUrlRef.current)
    }
  }, [])

  useEffect(() => {
    Promise.all([launchOptions(), overlayExclusion()])
      .then(([loadedOptions, loadedExclusion]) => {
        setOverlayMode(loadedExclusion)
        return startCapture(loadedOptions)
      })
      .then(() => setMessage('Select a region'))
      .catch((error) => {
        setStartupFailed(true)
        setStatus({ state: 'failed', message: String(error) })
        setMessage(String(error))
      })
  }, [])

  useEffect(() => {
    if (startupFailed) return
    const timer = window.setInterval(async () => {
      if (pollInFlightRef.current) return
      pollInFlightRef.current = true
      try {
        const nextStatus = await sessionStatus()
        setStatus(nextStatus)
        if (nextStatus.state === 'stitching') {
          const blob = await getStitchPreview(700)
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
        pollInFlightRef.current = false
      }
    }, 160)

    return () => window.clearInterval(timer)
  }, [startupFailed])

  const onSelect = useCallback(async (region: SourceRegion) => {
    try {
      setSelectedRegion(region)
      const confirmed = await confirmRegion(region)
      setMessage(`${confirmed.width}x${confirmed.height} selected`)
      await startStitching()
      setMessage('Scroll now')
    } catch (error) {
      setMessage(String(error))
    }
  }, [])

  const onCancel = useCallback(async () => {
    try {
      await stopCapture()
    } finally {
      window.close()
    }
  }, [])

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        onCancel()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [onCancel])

  const onStop = useCallback(async () => {
    try {
      const done = await stopStitching()
      setMessage(`Stitched ${done.image_width}x${done.image_height}`)
      const blob = await getFinalPreview(1400)
      if (blob) {
        const nextUrl = URL.createObjectURL(blob)
        setFinalPreviewUrl((oldUrl) => {
          if (oldUrl) URL.revokeObjectURL(oldUrl)
          return nextUrl
        })
      }
    } catch (error) {
      setMessage(String(error))
    }
  }, [])

  const onSave = useCallback(async () => {
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
  }, [])

  const activeRegion = selectedRegion ?? (status.state === 'stitching' ? status.region : null)
  const sourceWidth = status.state === 'previewing' || status.state === 'stitching' ? status.frame_width : 1
  const sourceHeight = status.state === 'previewing' || status.state === 'stitching' ? status.frame_height : 1
  const showSelection = status.state === 'previewing' || status.state === 'stitching'
  const canEditSelection = status.state === 'previewing'

  const placement = useMemo(() => {
    if (!activeRegion) {
      return { mode: 'status' } as const
    }
    const bounds = { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight }
    const regionRect = sourceRegionToCssRect(activeRegion, {
      renderedWidth: bounds.width,
      renderedHeight: bounds.height,
      sourceWidth,
      sourceHeight,
    })
    return choosePreviewPlacement({
      bounds,
      region: regionRect,
      preview: PREVIEW_SIZE,
      overlayExclusion: overlayMode,
    })
  }, [activeRegion, overlayMode, sourceHeight, sourceWidth])

  const toolbarMode = status.state === 'done' ? 'done' : status.state === 'failed' ? 'failed' : 'stitching'
  const stats =
    status.state === 'stitching'
      ? `${status.stats.frame_count} frames - ${status.stats.total_width}x${status.stats.total_height}px`
      : message

  return (
    <main className="capture-overlay">
      {status.state === 'done' && finalPreviewUrl ? (
        <img className="final-overlay-preview" src={finalPreviewUrl} alt="Stitched result" draggable={false} />
      ) : null}
      {showSelection ? (
        <SelectionLayer
          sourceWidth={sourceWidth}
          sourceHeight={sourceHeight}
          selectedRegion={activeRegion}
          disabled={!canEditSelection}
          onSelect={onSelect}
          onCancel={onCancel}
        />
      ) : status.state !== 'done' ? (
        <div className="selection-layer">
          <div className="selection-dim" />
          <div className="capture-status">{message}</div>
        </div>
      ) : null}
      {status.state === 'stitching' ? (
        <AdaptiveStitchPreview imageUrl={stitchPreviewUrl} status={stats} placement={placement} />
      ) : null}
      {status.state === 'stitching' || status.state === 'done' || status.state === 'failed' ? (
        <OverlayToolbar
          mode={toolbarMode}
          message={status.state === 'failed' ? status.message : stats}
          onStop={onStop}
          onSave={onSave}
          onClose={onCancel}
        />
      ) : null}
    </main>
  )
}

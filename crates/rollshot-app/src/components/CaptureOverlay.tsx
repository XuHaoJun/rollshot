import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  confirmRegion,
  getFinalPreview,
  getStitchPreview,
  launchOptions,
  overlayExclusion,
  setInputPassthrough,
  sessionStatus,
  startCapture,
  startStitching,
  stopCapture,
  stopStitching,
  type OverlayExclusion,
  type SessionStatus,
} from '../api/capture'
import { promptSaveStitchedPng } from '../api/save'
import type { PreviewScale, SourceRegion } from '../region/geometry'
import { sourceRegionToCssRect } from '../region/geometry'
import { choosePreviewPlacement, fitPreviewSizeToRegion } from '../overlay/placement'
import { AdaptiveStitchPreview } from './AdaptiveStitchPreview'
import { OverlayToolbar } from './OverlayToolbar'
import { SelectionLayer } from './SelectionLayer'

const MAX_PREVIEW_SIZE = { width: 180, height: 260 }
const OVERLAY_CLEAR_DELAY_MS = 17

let overlayStarted = false

export function resetOverlayStartedForTest() {
  overlayStarted = false
}

function waitForOverlayClear() {
  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, OVERLAY_CLEAR_DELAY_MS)
  })
}

export function CaptureOverlay() {
  const [status, setStatus] = useState<SessionStatus>({ state: 'idle' })
  const [overlayMode, setOverlayMode] = useState<OverlayExclusion>('unknown')
  const [selectedRegion, setSelectedRegion] = useState<SourceRegion | null>(null)
  const [isStartingStitching, setIsStartingStitching] = useState(false)
  const [stitchPreviewUrl, setStitchPreviewUrl] = useState<string | null>(null)
  const [finalPreviewUrl, setFinalPreviewUrl] = useState<string | null>(null)
  const [windowOrigin, setWindowOrigin] = useState<{ x: number; y: number } | null>(null)
  const [devicePixelRatio, setDevicePixelRatio] = useState<number>(() =>
    typeof window === 'undefined' ? 1 : window.devicePixelRatio,
  )
  const [message, setMessage] = useState('Starting capture')
  const [startupFailed, setStartupFailed] = useState(false)
  const [captureMissToast, setCaptureMissToast] = useState<string | null>(null)
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
    if (overlayStarted) {
      return
    }
    overlayStarted = true
    const tauriWindow = getCurrentWindow()
    Promise.all([
      launchOptions(),
      overlayExclusion(),
      tauriWindow.outerPosition(),
      tauriWindow.scaleFactor(),
    ])
      .then(([loadedOptions, loadedExclusion, outerPosition, scaleFactor]) => {
        setOverlayMode(loadedExclusion)
        setWindowOrigin({ x: outerPosition.x, y: outerPosition.y })
        setDevicePixelRatio(scaleFactor)
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
          const previewSize = fitPreviewSizeToRegion({
            region: nextStatus.region,
            maxPreview: MAX_PREVIEW_SIZE,
          })
          const blob = await getStitchPreview(previewSize.width, previewSize.height)
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

  // Show the toast when a warning pulse arrives.
  useEffect(() => {
    if (status.state === 'stitching' && status.capture_miss_warning) {
      setCaptureMissToast(status.capture_miss_message)
    }
  }, [status])

  // Dismiss it ~3s after it was last (re)shown. Keyed on the toast value, NOT on
  // `status`, so an intervening poll that flips warn->false cannot cancel it.
  useEffect(() => {
    if (!captureMissToast) return
    const timer = window.setTimeout(() => setCaptureMissToast(null), 3000)
    return () => window.clearTimeout(timer)
  }, [captureMissToast])

  const onSelect = useCallback(async (region: SourceRegion) => {
    try {
      setSelectedRegion(region)
      setIsStartingStitching(true)
      const confirmed = await confirmRegion(region)
      setMessage(`${confirmed.width}x${confirmed.height} selected`)
      await waitForOverlayClear()
      await startStitching()
      setMessage('Scroll now')
    } catch (error) {
      setIsStartingStitching(false)
      setMessage(String(error))
    }
  }, [])

  const closeOverlay = useCallback(async () => {
    try {
      await setInputPassthrough(false)
      await stopCapture()
    } finally {
      window.close()
    }
  }, [])

  const onCancel = useCallback(async () => {
    await closeOverlay()
  }, [closeOverlay])

  const finishStitching = useCallback(async () => {
    try {
      const done = await stopStitching()
      await setInputPassthrough(false)
      setMessage(`Stitched ${done.image_width}x${done.image_height}`)
      const blob = await getFinalPreview(1400)
      if (blob) {
        const nextUrl = URL.createObjectURL(blob)
        setFinalPreviewUrl((oldUrl) => {
          if (oldUrl) URL.revokeObjectURL(oldUrl)
          return nextUrl
        })
      }
      return true
    } catch (error) {
      setMessage(String(error))
      return false
    }
  }, [])

  const saveCurrentImage = useCallback(async (closeAfter: boolean) => {
    try {
      await promptSaveStitchedPng(setMessage)
      if (closeAfter) {
        await closeOverlay()
      }
    } catch (error) {
      setMessage(String(error))
    }
  }, [closeOverlay])

  const onSave = useCallback(async () => {
    await saveCurrentImage(false)
  }, [saveCurrentImage])

  const onStop = useCallback(async () => {
    await finishStitching()
  }, [finishStitching])

  const finishAndSave = useCallback(async () => {
    if (status.state === 'stitching') {
      const finished = await finishStitching()
      if (!finished) return
    }
    await saveCurrentImage(true)
  }, [finishStitching, saveCurrentImage, status.state])

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape' && (status.state === 'stitching' || status.state === 'done')) {
        event.preventDefault()
        finishAndSave()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [finishAndSave, status.state])

  useEffect(() => {
    if (status.state !== 'stitching') {
      return
    }
    setIsStartingStitching(false)
    setInputPassthrough(true).catch((error) => setMessage(String(error)))
    return () => {
      setInputPassthrough(false).catch((error) => setMessage(String(error)))
    }
  }, [status.state])

  const activeRegion = selectedRegion ?? (status.state === 'stitching' ? status.region : null)
  const hasCaptureFrame = status.state === 'previewing' || status.state === 'stitching'
  const sourceWidth = hasCaptureFrame ? status.frame_width : 1
  const sourceHeight = hasCaptureFrame ? status.frame_height : 1
  const showSelection = status.state === 'previewing' && !isStartingStitching

  const scale = useMemo<PreviewScale | null>(() => {
    if (!hasCaptureFrame || !windowOrigin) return null
    return {
      scaleX: devicePixelRatio,
      scaleY: devicePixelRatio,
      sourceOriginX: windowOrigin.x,
      sourceOriginY: windowOrigin.y,
      sourceWidth,
      sourceHeight,
    }
  }, [devicePixelRatio, hasCaptureFrame, sourceHeight, sourceWidth, windowOrigin])

  const activeRegionRect = useMemo(() => {
    if (!activeRegion || !scale) return null
    return sourceRegionToCssRect(activeRegion, scale)
  }, [activeRegion, scale])

  const placement = useMemo(() => {
    if (!activeRegionRect) {
      return { mode: 'status' } as const
    }
    const bounds = { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight }
    const previewSize = fitPreviewSizeToRegion({
      region: activeRegionRect,
      maxPreview: MAX_PREVIEW_SIZE,
    })
    return choosePreviewPlacement({
      bounds,
      region: activeRegionRect,
      preview: previewSize,
      overlayExclusion: overlayMode,
    })
  }, [activeRegionRect, overlayMode])

  const toolbarMode = status.state === 'done' ? 'done' : status.state === 'failed' ? 'failed' : 'stitching'
  const stats =
    status.state === 'stitching'
      ? `${status.stats.frame_count} frames - ${status.stats.total_width}x${status.stats.total_height}px`
      : message

  return (
    <main className="capture-overlay">
      {(status.state === 'stitching' || isStartingStitching) && activeRegionRect ? (
        <div
          className="capture-mask"
          style={{
            left: `${activeRegionRect.left}px`,
            top: `${activeRegionRect.top}px`,
            width: `${activeRegionRect.width}px`,
            height: `${activeRegionRect.height}px`,
          }}
        />
      ) : null}
      {status.state === 'done' && finalPreviewUrl ? (
        <img className="final-overlay-preview" src={finalPreviewUrl} alt="Stitched result" draggable={false} />
      ) : null}
      {showSelection && scale ? (
        <SelectionLayer
          scale={scale}
          selectedRegion={activeRegion}
          onSelect={onSelect}
          onCancel={onCancel}
        />
      ) : status.state !== 'done' && status.state !== 'stitching' ? (
        <div className="selection-layer">
          <div className="selection-dim" />
          <div className="capture-status">{message}</div>
        </div>
      ) : null}
      {status.state === 'stitching' ? (
        <AdaptiveStitchPreview
          imageUrl={stitchPreviewUrl}
          status={stats}
          placement={placement}
          processing={status.state === 'stitching'}
        />
      ) : null}
      {captureMissToast ? <div className="capture-miss-toast">{captureMissToast}</div> : null}
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

import { invoke } from '@tauri-apps/api/core'
import type { SourceRegion } from '../region/geometry'

export type OverlayMode = 'auto' | 'tauri' | 'iced'

export type InteractiveLaunchOptions = {
  backend: string
  fps: number
  show_cursor: boolean
  overlay_mode: OverlayMode
}

export type RegionDto = {
  x: number
  y: number
  width: number
  height: number
}

export type StitchStatsDto = {
  frame_count: number
  total_width: number
  total_height: number
  last_append: number
}

export type DoneImageDto = {
  image_width: number
  image_height: number
  output_path: string | null
}

export type CapturedEdge = 'top' | 'bottom' | 'left' | 'right' | 'unknown'

export type SessionStatus =
  | { state: 'idle' }
  | {
      state: 'previewing'
      frame_width: number
      frame_height: number
      region: RegionDto | null
    }
  | {
      state: 'stitching'
      frame_width: number
      frame_height: number
      region: RegionDto
      stats: StitchStatsDto
      last_outcome: string | null
      capture_miss: boolean
      capture_miss_warning: boolean
      capture_miss_edge: CapturedEdge
      capture_miss_message: string
    }
  | {
      state: 'done'
      image_width: number
      image_height: number
      output_path: string | null
    }
  | { state: 'failed'; message: string }

export async function launchOptions(): Promise<InteractiveLaunchOptions> {
  return await invoke<InteractiveLaunchOptions>('launch_options')
}

export async function startCapture(options: InteractiveLaunchOptions): Promise<void> {
  await invoke('start_capture', { options })
}

export async function stopCapture(): Promise<void> {
  await invoke('stop_capture')
}

export async function sessionStatus(): Promise<SessionStatus> {
  return await invoke<SessionStatus>('session_status')
}

export async function getLatestPreview(maxEdge: number): Promise<Blob | null> {
  const bytes = await invoke<ArrayBuffer>('get_latest_preview', { maxEdge })
  if (bytes.byteLength === 0) {
    return null
  }
  return new Blob([bytes], { type: 'image/png' })
}

export async function confirmRegion(region: SourceRegion): Promise<RegionDto> {
  return await invoke<RegionDto>('confirm_region', {
    region: {
      x: region.x,
      y: region.y,
      width: region.width,
      height: region.height,
    },
  })
}

export async function startStitching(): Promise<void> {
  await invoke('start_stitching')
}

export async function stopStitching(): Promise<DoneImageDto> {
  return await invoke<DoneImageDto>('stop_stitching')
}

export async function saveImage(path: string): Promise<DoneImageDto> {
  return await invoke<DoneImageDto>('save_image', { path })
}

export async function runNativeCapture(
  options: InteractiveLaunchOptions,
): Promise<DoneImageDto | null> {
  return await invoke<DoneImageDto | null>('run_native_capture', { options })
}

export async function usesNativeOverlay(): Promise<boolean> {
  return await invoke<boolean>('uses_native_overlay')
}

export async function exitApp(): Promise<void> {
  await invoke('exit_app')
}

export async function getStitchPreview(previewWidth: number, previewHeight: number): Promise<Blob | null> {
  const bytes = await invoke<ArrayBuffer>('get_stitch_preview', { previewWidth, previewHeight })
  if (bytes.byteLength === 0) {
    return null
  }
  return new Blob([bytes], { type: 'image/png' })
}

export async function getFinalPreview(maxEdge: number): Promise<Blob | null> {
  const bytes = await invoke<ArrayBuffer>('get_final_preview', { maxEdge })
  if (bytes.byteLength === 0) {
    return null
  }
  return new Blob([bytes], { type: 'image/png' })
}

export type OverlayExclusion = 'verified' | 'unsupported' | 'unknown'

export async function overlayExclusion(): Promise<OverlayExclusion> {
  return await invoke<OverlayExclusion>('overlay_exclusion')
}

export async function setInputPassthrough(enabled: boolean): Promise<void> {
  await invoke('set_input_passthrough', { enabled })
}

import { useEffect, useState } from 'react'
import { usesNativeOverlay } from './api/capture'
import { CaptureOverlay } from './components/CaptureOverlay'
import { NativeCaptureFlow } from './components/NativeCaptureFlow'

type CaptureMode = 'loading' | 'native' | 'webview'

export default function App() {
  const [mode, setMode] = useState<CaptureMode>('loading')

  useEffect(() => {
    usesNativeOverlay()
      .then((native) => setMode(native ? 'native' : 'webview'))
      .catch(() => setMode('webview'))
  }, [])

  if (mode === 'loading') {
    return null
  }
  return mode === 'native' ? <NativeCaptureFlow /> : <CaptureOverlay />
}

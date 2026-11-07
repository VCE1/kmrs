import { useEffect } from 'react'
import { setThemeColorOverride } from '@/lib/store/ui'

export function useReaderViewportBackground(color: string | null) {
  useEffect(() => {
    if (color === null) return

    const previousBackground = document.body.style.backgroundColor
    document.body.style.backgroundColor = color
    setThemeColorOverride(color)

    return () => {
      document.body.style.backgroundColor = previousBackground
      setThemeColorOverride(null)
    }
  }, [color])
}

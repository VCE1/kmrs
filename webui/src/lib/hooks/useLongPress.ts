import { useEffect, useRef, type MouseEvent, type PointerEvent } from 'react'

/** Touch-only so mouse clicks and desktop context menus keep their usual behavior. */
export function useLongPress(onLongPress: () => void) {
  const callback = useRef(onLongPress)
  useEffect(() => {
    callback.current = onLongPress
  }, [onLongPress])
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const start = useRef({ x: 0, y: 0 })
  const pressed = useRef(false)
  const fired = useRef(false)

  const cancel = () => {
    if (timer.current) clearTimeout(timer.current)
    timer.current = null
    pressed.current = false
  }

  useEffect(() => cancel, [])

  return {
    onPointerDown: (e: PointerEvent<HTMLElement>) => {
      if (e.pointerType !== 'touch' || !e.isPrimary) return
      cancel()
      pressed.current = true
      start.current = { x: e.clientX, y: e.clientY }
      timer.current = setTimeout(() => {
        timer.current = null
        fired.current = true
        callback.current()
      }, 500)
    },
    onPointerMove: (e: PointerEvent<HTMLElement>) => {
      if (!pressed.current) return
      if (Math.hypot(e.clientX - start.current.x, e.clientY - start.current.y) > 10) cancel()
    },
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onClickCapture: (e: MouseEvent<HTMLElement>) => {
      if (!fired.current) return
      fired.current = false
      e.preventDefault()
      e.stopPropagation()
      pressed.current = false
    },
    onContextMenu: (e: MouseEvent<HTMLElement>) => {
      if (window.matchMedia('(pointer: coarse)').matches) e.preventDefault()
    },
  }
}

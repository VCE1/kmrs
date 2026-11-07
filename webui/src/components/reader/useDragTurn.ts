import { useEffect, useRef } from 'react'

/** travel before the gesture axis locks; below it the trailing click stays a tap */
const AXIS_LOCK_PX = 8
/** turn threshold for a release-only swipe (the modes the drag cannot follow) */
const SWIPE_MIN_PX = 48
/** a followed drag turns past its commit distance (see COMMIT_MAX_PX), or on a fast enough flick */
const FLICK_PX_PER_MS = 0.5
const FLICK_MIN_PX = 24
/** caps the commit distance so large desktop windows keep the drag a short flick-like gesture */
const COMMIT_MAX_PX = 180
/** matches the track's turn transition, so a committed pull leaves after the slide-off settles */
const SETTLE_MS = 300

interface DragTurnOptions {
  /** the track element whose transform the drag rewrites; frames go straight to style so a drag never re-renders */
  track: () => HTMLDivElement | null
  vertical: boolean
  flip: boolean
  /** false when the turn axis scrolls natively or pages crossfade: the swipe stays release-only */
  follow: boolean
  animate: boolean
  /** the slot the track rests at: the spread index, or one past either end while a transition page shows */
  resting: number
  spreadCount: number
  /** a turn that only swaps a split wide page's half cannot be dragged, so that direction stays release-only */
  followForward: boolean
  followBackward: boolean
  /** viewport fraction past which a followed drag commits; denser layouts commit earlier */
  commitRatio: number
  swipe: boolean
  onNext: () => void
  onPrevious: () => void
  /** a committed pull at a boundary leaves the book directly, skipping the transition page's rest */
  onLeave: (dir: 'previous' | 'next') => void
  /** true once a pinch/pan owns the gesture */
  gestureConsumed: () => boolean
  /** swallow the click the browser fires after pointerup */
  markSwiped: () => void
}

type Status = 'idle' | 'pending' | 'drag' | 'release' | 'settling'

/**
 * Drag-to-turn for the paged reader. The drag follows the finger one slot in either direction —
 * past the first/last spread the track's transition page is what gets revealed, and committing
 * that pull leaves the book straight away (the tap/wheel path keeps its two-step transition
 * page). Where the drag cannot follow (native scroll on the turn axis, crossfade, split-half
 * turns), the release-only swipe keeps its previous thresholds.
 */
export function useDragTurn(options: DragTurnOptions) {
  const opts = useRef(options)
  useEffect(() => {
    opts.current = options
  })
  const state = useRef({
    status: 'idle' as Status,
    x: 0,
    y: 0,
    size: 1,
    delta: 0,
    velocity: 0,
    lastT: 0,
    lastDelta: 0,
    timer: undefined as number | undefined,
  })
  useEffect(() => () => window.clearTimeout(state.current.timer), [])

  /** signed slot delta per drag pixel: a forward drag carries this sign along the turn axis */
  const forwardSign = () => {
    const o = opts.current
    return o.vertical ? -1 : o.flip ? 1 : -1
  }

  const write = (slot: number, px: number, settling: boolean) => {
    const el = opts.current.track()
    if (!el) return
    el.classList.toggle('dragging', !settling)
    // matches offset(): LTR/vertical rest at -slot*100%, RTL at +slot*100%; the drag delta is
    // already signed along the turn axis, so it composes with the base unflipped
    const pct = forwardSign() * slot * 100
    el.style.transform = `translate${opts.current.vertical ? 'Y' : 'X'}(calc(${pct}% + ${px}px))`
  }

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    const s = state.current
    const o = opts.current
    if (s.status === 'settling') return
    if (!o.swipe || !e.isPrimary || o.gestureConsumed()) return
    s.status = 'pending'
    s.x = e.clientX
    s.y = e.clientY
    s.size = o.vertical ? e.currentTarget.clientHeight : e.currentTarget.clientWidth
    s.delta = 0
    s.velocity = 0
    s.lastT = e.timeStamp
    s.lastDelta = 0
  }

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const s = state.current
    if ((s.status !== 'pending' && s.status !== 'drag') || !e.isPrimary) return
    const o = opts.current
    // a second finger turned this into a pinch: hand the gesture over and spring back
    if (o.gestureConsumed()) {
      if (s.status === 'drag') write(o.resting, 0, true)
      s.status = 'idle'
      return
    }
    const d = (o.vertical ? e.clientY : e.clientX) - (o.vertical ? s.y : s.x)
    const cross = (o.vertical ? e.clientX : e.clientY) - (o.vertical ? s.x : s.y)
    if (s.status === 'pending') {
      if (Math.max(Math.abs(d), Math.abs(cross)) < AXIS_LOCK_PX) return
      // the cross axis wins: a native scroll or no turn at all
      if (Math.abs(cross) > Math.abs(d)) {
        s.status = 'idle'
        return
      }
      const forward = Math.sign(d) === forwardSign()
      const room = forward ? Math.min(1, o.spreadCount - o.resting) : Math.min(1, o.resting + 1)
      s.status = o.follow && (forward ? o.followForward : o.followBackward) && room > 0 ? 'drag' : 'release'
      if (s.status !== 'drag') return
    }
    const room = (fwd: boolean) => (fwd ? Math.min(1, o.spreadCount - o.resting) : Math.min(1, o.resting + 1))
    const slots = (d * forwardSign()) / s.size
    const clamped = Math.max(-room(false), Math.min(room(true), slots))
    const dc = clamped * s.size * forwardSign()
    const dt = e.timeStamp - s.lastT
    if (dt > 0) {
      s.velocity = s.velocity * 0.7 + ((dc - s.lastDelta) / dt) * 0.3
      s.lastT = e.timeStamp
      s.lastDelta = dc
    }
    s.delta = dc
    write(o.resting, dc, false)
  }

  const settle = (slot: number, action: (() => void) | null, delayAction: boolean) => {
    const s = state.current
    const o = opts.current
    s.status = 'settling'
    write(slot, 0, true)
    window.clearTimeout(s.timer)
    s.timer = window.setTimeout(
      () => {
        s.status = 'idle'
        action?.()
      },
      o.animate && delayAction ? SETTLE_MS : 0,
    )
  }

  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    const s = state.current
    const o = opts.current
    if (s.status === 'pending' || s.status === 'release') {
      s.status = 'idle'
      if (o.gestureConsumed()) return
      const d = (o.vertical ? e.clientY : e.clientX) - (o.vertical ? s.y : s.x)
      const cross = (o.vertical ? e.clientX : e.clientY) - (o.vertical ? s.x : s.y)
      if (Math.abs(d) < SWIPE_MIN_PX || Math.abs(d) < Math.abs(cross)) return
      o.markSwiped()
      if (Math.sign(d) === forwardSign()) o.onNext()
      else o.onPrevious()
      return
    }
    if (s.status !== 'drag') return
    const forward = s.delta !== 0 && Math.sign(s.delta) === forwardSign()
    const flick =
      Math.abs(s.velocity) >= FLICK_PX_PER_MS &&
      Math.abs(s.delta) >= FLICK_MIN_PX &&
      Math.sign(s.velocity) === Math.sign(s.delta)
    const threshold = Math.min(s.size * o.commitRatio, COMMIT_MAX_PX)
    const commit = Math.abs(s.delta) > threshold || flick
    o.markSwiped()
    if (!commit) {
      settle(o.resting, null, false)
      return
    }
    const target = o.resting + (forward ? 1 : -1)
    // a pull landing past either end skips the transition page's rest and leaves the book
    if (target === -1 || target === o.spreadCount) {
      settle(target, () => o.onLeave(target === -1 ? 'previous' : 'next'), true)
    } else {
      settle(target, forward ? o.onNext : o.onPrevious, false)
    }
  }

  const onPointerCancel = () => {
    const s = state.current
    if (s.status === 'drag') settle(opts.current.resting, null, false)
    else s.status = 'idle'
  }

  return { onPointerDown, onPointerMove, onPointerUp, onPointerCancel }
}

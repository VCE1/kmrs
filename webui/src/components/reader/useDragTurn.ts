import { useEffect, useRef } from 'react'

/** travel before the gesture axis locks; below it the trailing click stays a tap */
const AXIS_LOCK_PX = 8
/** turn threshold for a release-only swipe (the modes the drag cannot follow) */
const SWIPE_MIN_PX = 48
/** a fast flick turns without the full travel, in followed and release-only swipes alike */
const FLICK_PX_PER_MS = 0.2
const FLICK_MIN_PX = 16
/** release velocity is the trailing window's endpoints: a finger slows before it lifts, so a drag-long average under-reads the flick */
const VELOCITY_WINDOW_MS = 100
/** a release commits on where its velocity would carry the drag this far past the lift */
const PROJECTION_MS = 150
/** a followed drag commits past a quarter of the viewport */
const COMMIT_FRACTION = 0.25
/** caps the commit distance so double-page spreads and wide desktop windows never demand more than one honest drag */
const COMMIT_MAX_PX = 96
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
    trail: [] as { p: number; t: number }[],
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

  /** feeds the trailing velocity window; the release samples too, so a pause before lifting reads as no flick */
  const sample = (pos: number, t: number) => {
    const s = state.current
    s.trail.push({ p: pos, t })
    while (s.trail.length > 2 && t - s.trail[0].t > VELOCITY_WINDOW_MS) s.trail.shift()
    const oldest = s.trail[0]
    s.velocity = t > oldest.t ? (pos - oldest.p) / (t - oldest.t) : 0
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
    s.trail = [{ p: o.vertical ? e.clientY : e.clientX, t: e.timeStamp }]
  }

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const s = state.current
    if ((s.status !== 'pending' && s.status !== 'drag' && s.status !== 'release') || !e.isPrimary) return
    const o = opts.current
    // a second finger turned this into a pinch: hand the gesture over and spring back
    if (o.gestureConsumed()) {
      if (s.status === 'drag') write(o.resting, 0, true)
      s.status = 'idle'
      return
    }
    const pos = o.vertical ? e.clientY : e.clientX
    const d = pos - (o.vertical ? s.y : s.x)
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
    }
    // velocity is the trailing window of the raw position: the followed offset is
    // clamped, so measuring it would decay to zero while the drag sits at a boundary
    sample(pos, e.timeStamp)
    if (s.status !== 'drag') return
    const room = (fwd: boolean) => (fwd ? Math.min(1, o.spreadCount - o.resting) : Math.min(1, o.resting + 1))
    const slots = (d * forwardSign()) / s.size
    const clamped = Math.max(-room(false), Math.min(room(true), slots))
    const dc = clamped * s.size * forwardSign()
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
    if (s.status === 'pending' || s.status === 'release' || s.status === 'drag') {
      sample(o.vertical ? e.clientY : e.clientX, e.timeStamp)
    }
    if (s.status === 'pending' || s.status === 'release') {
      s.status = 'idle'
      if (o.gestureConsumed()) return
      const d = (o.vertical ? e.clientY : e.clientX) - (o.vertical ? s.y : s.x)
      const cross = (o.vertical ? e.clientX : e.clientY) - (o.vertical ? s.x : s.y)
      // a flick turns below the travel threshold, so a quick swipe works in the modes
      // the drag cannot follow too
      const flick =
        Math.abs(s.velocity) >= FLICK_PX_PER_MS &&
        Math.abs(d) >= FLICK_MIN_PX &&
        Math.sign(s.velocity) === Math.sign(d)
      if ((!flick && Math.abs(d) < SWIPE_MIN_PX) || Math.abs(d) < Math.abs(cross)) return
      o.markSwiped()
      if (Math.sign(d) === forwardSign()) o.onNext()
      else o.onPrevious()
      return
    }
    if (s.status !== 'drag') return
    const threshold = Math.min(s.size * COMMIT_FRACTION, COMMIT_MAX_PX)
    // the release commits on where its momentum would land the drag, not on how far the
    // drag traveled: a decisive swipe never needs the full distance, and a release
    // already moving back cancels the travel it earned
    const projected = s.delta + s.velocity * PROJECTION_MS
    const flick =
      Math.abs(s.velocity) >= FLICK_PX_PER_MS &&
      Math.abs(s.delta) >= FLICK_MIN_PX &&
      Math.sign(s.velocity) === Math.sign(s.delta)
    const turn = Math.abs(projected) > threshold ? projected : flick ? s.delta : 0
    o.markSwiped()
    if (!turn) {
      settle(o.resting, null, false)
      return
    }
    const forward = Math.sign(turn) === forwardSign()
    // the projection can point away from the drag: past a transition page there is no slot
    // to land on, and a split half swap stays in its slot, so the track must not slide
    if ((forward ? o.spreadCount - o.resting : o.resting + 1) <= 0) {
      settle(o.resting, null, false)
      return
    }
    if (!(forward ? o.followForward : o.followBackward)) {
      settle(o.resting, forward ? o.onNext : o.onPrevious, false)
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

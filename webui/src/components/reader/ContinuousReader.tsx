import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { ArrowDown, ArrowUp } from '@phosphor-icons/react'
import { useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { useReaderBackground, useReaderSettings } from '@/lib/store/readerSettings'
import { cn } from '@/lib/utils/cn'
import { slotAspectRatio, type SpreadPage } from '@/lib/utils/spreads'
import { useWindowKeys } from './keys'
import { PageImage } from './PageImage'
import { useEdgePull, type PullDirection } from './useEdgePull'
import { usePinchZoom } from './usePinchZoom'
import type { BookDto } from '@/lib/api/types'

interface ContinuousReaderProps {
  pages: SpreadPage[]
  page: number
  onPageChange: (page: number) => void
  onToggleChrome: () => void
  onJumpPrevious: () => void
  onJumpNext: () => void
  /** siblings the edge pull can land on; null means the pull exits the reader */
  previousBook: BookDto | null
  nextBook: BookDto | null
  /** a committed pull leaves the book directly, skipping the two-step boundary toast */
  onLeaveBook: (dir: 'previous' | 'next') => void
  onMeasure: (page: number, width: number, height: number) => void
}

export function ContinuousReader({
  pages,
  page,
  onPageChange,
  onToggleChrome,
  onJumpPrevious,
  onJumpNext,
  previousBook,
  nextBook,
  onLeaveBook,
  onMeasure,
}: ContinuousReaderProps) {
  const scale = useReaderSettings((s) => s.continuousScale)
  const padding = useReaderSettings((s) => s.continuousPadding)
  const margin = useReaderSettings((s) => s.continuousMargin)
  const animations = useReaderSettings((s) => s.animations)
  const swipe = useReaderSettings((s) => s.swipe)
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')

  const {
    targetRef,
    zoomed,
    touchAction,
    onPointerDown,
    onPointerMove,
    onPointerUp,
    onPointerCancel,
    gestureConsumed,
    consumeClick,
    reset: resetPinch,
  } = usePinchZoom({ zoomedTouchAction: 'pan-y' })
  // fit/padding changes shift the strip geometry and leave stale pan offsets behind
  useLayoutEffect(() => {
    resetPinch()
  }, [scale, padding, margin, resetPinch])

  const scrollRef = useRef<HTMLDivElement>(null)
  const currentRef = useRef(page)
  // scroll snapshot for boundary checks; refs only, the scroll path must not re-render
  const posRef = useRef({ top: 0, max: 1 })
  const lastKeyRef = useRef(0)
  const [seen, setSeen] = useState<boolean[]>(() => pages.map(() => false))

  const barPrevRef = useRef<HTMLDivElement>(null)
  const barNextRef = useRef<HTMLDivElement>(null)
  const pull = useEdgePull({
    scroller: () => scrollRef.current,
    bar: (dir) => (dir === 'next' ? barNextRef.current : barPrevRef.current),
    enabled: !zoomed && swipe,
    gestureConsumed,
    onLeave: onLeaveBook,
    resetKey: pages,
  })
  const light = useReaderBackground() === 'WHITE'

  const onPageChangeRef = useRef(onPageChange)
  useEffect(() => {
    onPageChangeRef.current = onPageChange
  })

  const markSeen = useCallback((i: number) => {
    setSeen((prev) => (prev[i] ? prev : prev.map((v, j) => (j === i ? true : v))))
  }, [])

  useEffect(() => {
    const root = scrollRef.current
    if (!root) return
    // current-page tracking stays at the viewport center, but images need a head start
    // before short pages scroll into view, especially with a mouse wheel
    const loader = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue
          markSeen(Number((entry.target as HTMLElement).dataset.page) - 1)
        }
      },
      { root, rootMargin: '100% 0px 200% 0px' },
    )
    const current = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue
          const n = Number((entry.target as HTMLElement).dataset.page)
          currentRef.current = n
          onPageChangeRef.current(n)
        }
      },
      { root, rootMargin: '-45% 0px -45% 0px' },
    )
    root.querySelectorAll('[data-page]').forEach((el) => {
      loader.observe(el)
      current.observe(el)
    })
    return () => {
      loader.disconnect()
      current.disconnect()
    }
  }, [pages, markSeen])

  // initial position + boundary snapshot, once placeholders have laid out
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    if (page !== 1) el.querySelector(`[data-page="${page}"]`)?.scrollIntoView({ block: 'start' })
    posRef.current = { top: el.scrollTop, max: el.scrollHeight - el.clientHeight }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // external jumps (slider, thumbnails) scroll instantly; observer-driven changes already match
  useEffect(() => {
    if (page === currentRef.current) return
    currentRef.current = page
    scrollRef.current?.querySelector(`[data-page="${page}"]`)?.scrollIntoView({ block: 'start' })
  }, [page])

  const onScroll = () => {
    const el = scrollRef.current
    if (el) posRef.current = { top: el.scrollTop, max: el.scrollHeight - el.clientHeight }
  }

  const smooth = animations && !reduceMotion
  const scrollStep = (dir: 1 | -1) => {
    const el = scrollRef.current
    if (!el) return
    el.scrollBy({ top: dir * el.clientHeight * 0.95, behavior: smooth ? 'smooth' : 'auto' })
  }
  const prev = () => {
    if (posRef.current.top > 1) scrollStep(-1)
    else onJumpPrevious()
  }
  const next = () => {
    if (posRef.current.top < posRef.current.max - 1) scrollStep(1)
    else onJumpNext()
  }

  useWindowKeys((e) => {
    const forward = e.key === ' ' || e.key === 'PageDown' || e.key === 'ArrowDown'
    const back = e.key === 'PageUp' || e.key === 'ArrowUp'
    if (!forward && !back) return
    e.preventDefault()
    const now = Date.now()
    if (now - lastKeyRef.current < 500) return
    lastKeyRef.current = now
    if (forward) next()
    else prev()
  })

  // click regions instead of overlay zones: overlays would swallow wheel scrolling
  const onClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (consumeClick() || pull.consumeClick()) return
    // while zoomed a tap only toggles chrome; a scroll jump would dump the zoom
    if (zoomed) {
      onToggleChrome()
      return
    }
    const rect = e.currentTarget.getBoundingClientRect()
    const ratio = (e.clientY - rect.top) / rect.height
    if (ratio < 0.25) prev()
    else if (ratio > 0.75) next()
    else onToggleChrome()
  }

  const pullLabel = (dir: PullDirection) => {
    const sibling = dir === 'next' ? nextBook : previousBook
    const release = pull.armed === dir
    if (!sibling) return t(release ? 'pull.exitRelease' : 'pull.exit')
    return t(release ? `pull.${dir}Release` : `pull.${dir}`)
  }

  const pullBar = (dir: PullDirection, ref: React.RefObject<HTMLDivElement | null>) => {
    const next = dir === 'next'
    const sibling = next ? nextBook : previousBook
    return (
      <div
        ref={ref}
        className={cn(
          'pointer-events-none absolute inset-x-0 z-10 flex h-14 flex-col items-center justify-center gap-0.5 backdrop-blur-sm',
          next ? 'bottom-0 border-t' : 'top-0 border-b',
          light ? 'border-black/10 bg-white/85 text-black' : 'border-white/10 bg-black/75 text-white',
          // the transform is written per frame while pulling; the transition only runs on snap-back
          '[&.snapping]:transition-transform [&.snapping]:duration-200',
        )}
        style={{ transform: `translateY(${next ? 56 : -56}px)` }}
      >
        <div
          className={cn(
            'flex items-center gap-1.5 text-[13px]',
            pull.armed === dir && 'font-semibold',
          )}
        >
          {next ? <ArrowDown aria-hidden className="size-4" /> : <ArrowUp aria-hidden className="size-4" />}
          <span>{pullLabel(dir)}</span>
        </div>
        {sibling && (
          <p className="max-w-[80%] truncate text-[11px] opacity-60">
            {sibling.metadata.title || sibling.name}
          </p>
        )}
      </div>
    )
  }

  return (
    <div
      className="relative h-full overflow-hidden"
      style={{ touchAction }}
      onClick={onClick}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
    >
      <div
        ref={targetRef}
        className="h-full"
        style={{ transformOrigin: '0 0', willChange: zoomed ? 'transform' : undefined }}
      >
        <div
          ref={scrollRef}
          onScroll={onScroll}
          onWheel={pull.onWheel}
          className="h-full overflow-y-auto overscroll-none no-scrollbar"
        >
          {pages.map((p, i) => {
            const load = i === 0 || seen[i] || Math.abs(i + 1 - page) <= 2
            return (
              <PageImage
                key={p.number}
                data-page={p.number}
                src={p.url}
                alt={t('pageAlt', { page: p.number })}
                load={load}
                className="mx-auto"
                imgClassName="block h-auto w-full"
                style={{
                  width: scale === 'WIDTH' ? `${100 - padding * 2}%` : (p.width ?? '100%'),
                  marginTop: i === 0 ? 0 : margin,
                  // reserve the slot's height before the image loads so the pages
                  // below do not shift when it arrives
                  aspectRatio: slotAspectRatio(p),
                }}
                onMeasure={(w, h) => onMeasure(p.number, w, h)}
              />
            )
          })}
        </div>
      </div>
      {pullBar('previous', barPrevRef)}
      {pullBar('next', barNextRef)}
    </div>
  )
}

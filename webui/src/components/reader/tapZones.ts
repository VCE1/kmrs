import type { TapZoneLayout } from '@/lib/store/readerSettings'

export type TapAction = 'previous' | 'next' | 'menu'

/**
 * `along` runs 0-1 on the page-turn axis in screen order and `across` on the other axis.
 * `flip` is right-to-left reading, so every layout keeps its meaning in reading order.
 */
export function tapAction(layout: TapZoneLayout, along: number, across: number, flip: boolean): TapAction {
  if (layout === 'OFF') return 'menu'
  const reading = flip ? 1 - along : along
  // the top strip keeps the menu reachable once the turn zones cover the whole page
  if (layout === 'KINDLE') {
    if (across < 0.25) return 'menu'
    return reading < 0.25 ? 'previous' : 'next'
  }
  const edge = layout === 'EDGE' ? 0.15 : 0.25
  if (reading >= edge && reading <= 1 - edge) return 'menu'
  const forward = reading > 0.5
  return forward !== (layout === 'REVERSED') ? 'next' : 'previous'
}

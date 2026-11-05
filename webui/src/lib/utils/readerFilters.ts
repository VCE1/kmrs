import type { CSSProperties } from 'react'

export type FilterBlendMode = 'normal' | 'multiply' | 'screen' | 'overlay' | 'darken' | 'lighten'

export const FILTER_BLEND_MODES: readonly FilterBlendMode[] = [
  'normal',
  'multiply',
  'screen',
  'overlay',
  'darken',
  'lighten',
]

export interface FilterLevel {
  enabled: boolean
  value: number
}

export interface TintFilter {
  enabled: boolean
  red: number
  green: number
  blue: number
  alpha: number
  blendMode: FilterBlendMode
}

export interface ReaderFilters {
  brightness: FilterLevel
  contrast: FilterLevel
  saturate: FilterLevel
  hue: FilterLevel
  sepia: boolean
  grayscale: boolean
  invert: boolean
  tint: TintFilter
}

export interface FilterLimit {
  min: number
  max: number
  step: number
  defaultValue: number
}

export const FILTER_LIMITS: Record<'brightness' | 'contrast' | 'saturate' | 'hue', FilterLimit> = {
  brightness: { min: 5, max: 200, step: 1, defaultValue: 100 },
  contrast: { min: 5, max: 200, step: 1, defaultValue: 100 },
  saturate: { min: 0, max: 200, step: 1, defaultValue: 100 },
  hue: { min: 0, max: 360, step: 1, defaultValue: 0 },
}

export const TINT_CHANNEL_LIMIT = { min: 0, max: 255, step: 1 }
export const TINT_ALPHA_LIMIT = { min: 0, max: 100, step: 1 }

export const DEFAULT_FILTERS: ReaderFilters = {
  brightness: { enabled: false, value: FILTER_LIMITS.brightness.defaultValue },
  contrast: { enabled: false, value: FILTER_LIMITS.contrast.defaultValue },
  saturate: { enabled: false, value: FILTER_LIMITS.saturate.defaultValue },
  hue: { enabled: false, value: FILTER_LIMITS.hue.defaultValue },
  sepia: false,
  grayscale: false,
  invert: false,
  tint: {
    enabled: false,
    red: 0,
    green: 0,
    blue: 0,
    alpha: 50,
    blendMode: 'normal',
  },
}

/** CSS `filter` value for page images; the tint is excluded because it renders as a blend overlay. */
export function buildFilterString(filters: ReaderFilters): string {
  const parts: string[] = []
  if (filters.brightness.enabled) parts.push(`brightness(${filters.brightness.value / 100})`)
  if (filters.contrast.enabled) parts.push(`contrast(${filters.contrast.value / 100})`)
  if (filters.saturate.enabled) parts.push(`saturate(${filters.saturate.value / 100})`)
  if (filters.hue.enabled) parts.push(`hue-rotate(${filters.hue.value}deg)`)
  if (filters.invert) parts.push('invert(1)')
  if (filters.sepia) parts.push('sepia(1)')
  if (filters.grayscale) parts.push('grayscale(1)')
  return parts.join(' ')
}

/**
 * CSS filter has no tint-with-blend-mode primitive, so the tint paints as a fixed
 * overlay div covering the reader viewport instead of joining the filter string.
 */
export function buildTintStyle(filters: ReaderFilters): Pick<CSSProperties, 'background' | 'mixBlendMode'> | null {
  const { tint } = filters
  if (!tint.enabled) return null
  return {
    background: `rgba(${tint.red}, ${tint.green}, ${tint.blue}, ${tint.alpha / 100})`,
    mixBlendMode: tint.blendMode,
  }
}

export function hasActiveFilters(filters: ReaderFilters): boolean {
  return (
    filters.brightness.enabled ||
    filters.contrast.enabled ||
    filters.saturate.enabled ||
    filters.hue.enabled ||
    filters.sepia ||
    filters.grayscale ||
    filters.invert ||
    filters.tint.enabled
  )
}

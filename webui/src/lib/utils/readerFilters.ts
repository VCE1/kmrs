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

/** id of the SVG filter that paints the color tint; the reader route renders its definition */
export const TINT_FILTER_ID = 'kmreader-tint'

/**
 * CSS `filter` value for page images. CSS has no tint-with-blend-mode filter function,
 * so the tint joins the chain as a reference to an SVG filter; it runs last, on top of
 * the other adjustments, and only touches the image's own pixels.
 */
export function buildFilterString(filters: ReaderFilters): string {
  const parts: string[] = []
  if (filters.brightness.enabled) parts.push(`brightness(${filters.brightness.value / 100})`)
  if (filters.contrast.enabled) parts.push(`contrast(${filters.contrast.value / 100})`)
  if (filters.saturate.enabled) parts.push(`saturate(${filters.saturate.value / 100})`)
  if (filters.hue.enabled) parts.push(`hue-rotate(${filters.hue.value}deg)`)
  if (filters.invert) parts.push('invert(1)')
  if (filters.sepia) parts.push('sepia(1)')
  if (filters.grayscale) parts.push('grayscale(1)')
  if (filters.tint.enabled) parts.push(`url(#${TINT_FILTER_ID})`)
  return parts.join(' ')
}

/**
 * Fill persisted filters up to the current shape: zustand's persist merge is shallow,
 * so fields added to the nested filter objects later would otherwise stay undefined.
 */
export function normalizeFilters(persisted: unknown): ReaderFilters {
  const f = (persisted ?? {}) as Partial<{ [K in keyof ReaderFilters]: Partial<ReaderFilters[K]> }>
  const level = (key: keyof typeof FILTER_LIMITS): FilterLevel => ({ ...DEFAULT_FILTERS[key], ...f[key] })
  return {
    brightness: level('brightness'),
    contrast: level('contrast'),
    saturate: level('saturate'),
    hue: level('hue'),
    sepia: typeof f.sepia === 'boolean' ? f.sepia : DEFAULT_FILTERS.sepia,
    grayscale: typeof f.grayscale === 'boolean' ? f.grayscale : DEFAULT_FILTERS.grayscale,
    invert: typeof f.invert === 'boolean' ? f.invert : DEFAULT_FILTERS.invert,
    tint: { ...DEFAULT_FILTERS.tint, ...f.tint },
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

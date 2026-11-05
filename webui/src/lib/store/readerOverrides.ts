import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { useReaderSettings, type ReaderSettings } from '@/lib/store/readerSettings'
import { DEFAULT_FILTERS, type ReaderFilters } from '@/lib/utils/readerFilters'

/**
 * Settings a series can override without touching the global defaults. Only
 * content-dependent options are overridable; personal preferences (background,
 * animations, transitions, tap zones) stay global so they behave the same
 * everywhere. Filters have no global default at all: they are content
 * processing tuned per series, and the fallback is no filters.
 */
export const OVERRIDABLE_KEYS = ['readingDirection', 'pageLayout', 'scale', 'splitWide', 'filters'] as const

export type OverridableKey = (typeof OVERRIDABLE_KEYS)[number]

export type ReaderOverride = Partial<
  Pick<ReaderSettings, Exclude<OverridableKey, 'filters'>> & { filters: ReaderFilters }
>

interface OverrideEntry {
  override: ReaderOverride
  updatedAt: number
}

// libraries can hold thousands of series; overrides accumulate one entry per
// visited series, so the persisted map is pruned to the most recently used ones
const MAX_OVERRIDES = 200

/** structural equality that does not depend on property order */
function stableStringify(value: unknown): string {
  if (value === null || typeof value !== 'object') return JSON.stringify(value) ?? 'null'
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(',')}]`
  return `{${Object.entries(value as Record<string, unknown>)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([k, v]) => `${JSON.stringify(k)}:${stableStringify(v)}`)
    .join(',')}}`
}

/** primitives compare directly; the filters object compares structurally */
function valuesEqual(a: unknown, b: unknown): boolean {
  return a === b || stableStringify(a) === stableStringify(b)
}

/**
 * What a key falls back to without an override: the global default for the
 * layout options, DEFAULT_FILTERS for filters since they have no global
 * default. The reading direction is excluded from this comparison — its
 * baseline is the resolved direction (series metadata, else global), which
 * only the caller knows.
 */
function fallbackFor(key: OverridableKey, global: ReaderSettings): unknown {
  return key === 'filters' ? DEFAULT_FILTERS : global[key]
}

interface ReaderOverridesState {
  bySeries: Record<string, OverrideEntry>
  /**
   * Merge a patch into a series' override. Undefined values — and values equal
   * to the key's fallback — delete their key, so the series follows the
   * fallback again; an emptied entry disappears entirely.
   */
  update: (seriesId: string, patch: ReaderOverride) => void
  /** refresh the LRU timestamp when a series with overrides is opened */
  touch: (seriesId: string) => void
  /** drop every override of one series */
  reset: (seriesId: string) => void
}

export const useReaderOverrides = create<ReaderOverridesState>()(
  persist(
    (set) => ({
      bySeries: {},
      update: (seriesId, patch) => {
        set((s) => {
          const global = useReaderSettings.getState()
          const merged: ReaderOverride = { ...s.bySeries[seriesId]?.override }
          for (const key of OVERRIDABLE_KEYS) {
            if (!(key in patch)) continue
            const value = patch[key]
            if (value === undefined || (key !== 'readingDirection' && valuesEqual(value, fallbackFor(key, global))))
              delete merged[key]
            else Object.assign(merged, { [key]: value })
          }
          const bySeries = { ...s.bySeries }
          if (Object.keys(merged).length === 0) delete bySeries[seriesId]
          else bySeries[seriesId] = { override: merged, updatedAt: Date.now() }
          const ids = Object.keys(bySeries)
          if (ids.length > MAX_OVERRIDES) {
            ids
              .sort((a, b) => bySeries[a].updatedAt - bySeries[b].updatedAt)
              .slice(0, ids.length - MAX_OVERRIDES)
              .forEach((id) => delete bySeries[id])
          }
          return { bySeries }
        })
      },
      touch: (seriesId) =>
        set((s) => {
          const entry = s.bySeries[seriesId]
          if (!entry) return s
          return { bySeries: { ...s.bySeries, [seriesId]: { ...entry, updatedAt: Date.now() } } }
        }),
      reset: (seriesId) =>
        set((s) => {
          if (!(seriesId in s.bySeries)) return s
          const bySeries = { ...s.bySeries }
          delete bySeries[seriesId]
          return { bySeries }
        }),
    }),
    {
      name: 'kmweb.reader.overrides',
      // v1: per-book entries predate the series granularity and cannot be remapped
      version: 1,
      migrate: () => ({ bySeries: {} }),
    },
  ),
)

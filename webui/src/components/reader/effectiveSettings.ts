import { createContext, useContext, useEffect, useMemo } from 'react'
import type { ReadingDirection } from '@/lib/api/types'
import { useReaderSettings, type ReaderSettings } from '@/lib/store/readerSettings'
import { useReaderOverrides, type OverridableKey, type ReaderOverride } from '@/lib/store/readerOverrides'
import { buildFilterString, normalizeFilters, type ReaderFilters } from '@/lib/utils/readerFilters'

export type EffectiveReaderSettings = Pick<ReaderSettings, Exclude<OverridableKey, 'filters'>> & {
  filters: ReaderFilters
}

export interface EffectiveReaderSettingsValue {
  settings: EffectiveReaderSettings
  /** keys the current series overrides; drives the "custom settings" affordance in the panel */
  overriddenKeys: OverridableKey[]
  /** writes a per-series override; undefined values delete their key */
  update: (patch: ReaderOverride) => void
  resetOverrides: () => void
}

export const EffectiveReaderSettingsContext = createContext<EffectiveReaderSettingsValue | null>(null)

/**
 * Merges the global defaults with the current series' overrides. The reading
 * direction is the only three-level key: per-series override, then the series
 * metadata direction, then the global default. Filters have no global default;
 * without an override the series gets DEFAULT_FILTERS via normalizeFilters.
 */
export function useEffectiveReaderSettings(
  seriesId: string,
  metaDirection: ReadingDirection | null,
): EffectiveReaderSettingsValue {
  const override = useReaderOverrides((s) => s.bySeries[seriesId]?.override)
  const updateStore = useReaderOverrides((s) => s.update)
  const touch = useReaderOverrides((s) => s.touch)
  const resetStore = useReaderOverrides((s) => s.reset)
  const globals = useReaderSettings()
  // opening a series keeps its override inside the LRU window
  useEffect(() => {
    if (seriesId) touch(seriesId)
  }, [seriesId, touch])
  return useMemo<EffectiveReaderSettingsValue>(
    () => ({
      settings: {
        readingDirection: override?.readingDirection ?? metaDirection ?? globals.readingDirection,
        pageLayout: override?.pageLayout ?? globals.pageLayout,
        scale: override?.scale ?? globals.scale,
        splitWide: override?.splitWide ?? globals.splitWide,
        filters: normalizeFilters(override?.filters),
      },
      overriddenKeys: (Object.keys(override ?? {}) as OverridableKey[]).filter((k) => override?.[k] !== undefined),
      update: (patch: ReaderOverride) => updateStore(seriesId, patch),
      resetOverrides: () => resetStore(seriesId),
    }),
    [override, metaDirection, globals, seriesId, updateStore, resetStore],
  )
}

function useEffective(): EffectiveReaderSettingsValue {
  const ctx = useContext(EffectiveReaderSettingsContext)
  if (!ctx) throw new Error('reader effective settings used outside the reader route')
  return ctx
}

export function useEffectiveSettings(): EffectiveReaderSettings {
  return useEffective().settings
}

export function useEffectiveControls(): Pick<
  EffectiveReaderSettingsValue,
  'overriddenKeys' | 'update' | 'resetOverrides'
> {
  const { overriddenKeys, update, resetOverrides } = useEffective()
  return { overriddenKeys, update, resetOverrides }
}

export function useEffectivePageFilter(): string {
  const { filters } = useEffectiveSettings()
  return useMemo(() => buildFilterString(filters), [filters])
}

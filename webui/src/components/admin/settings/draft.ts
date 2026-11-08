import i18n from '@/lib/i18n'
import type { SettingsDto, SettingsUpdateDto, ThumbnailSize } from '@/lib/api/types'

/** Editable form mirror of SettingsDto: numeric inputs stay strings until saved. */
export interface SettingsDraft {
  deleteEmptyCollections: boolean
  deleteEmptyReadLists: boolean
  rememberMeDurationDays: string
  thumbnailSize: ThumbnailSize
  taskPoolSize: string
  koboProxy: boolean
  skipReadOnNext: boolean
  /** days as a string to match the segmented control; '0' = off */
  recentReadingDays: string
}

export function draftFromSettings(s: SettingsDto): SettingsDraft {
  return {
    deleteEmptyCollections: s.deleteEmptyCollections ?? false,
    deleteEmptyReadLists: s.deleteEmptyReadLists ?? false,
    rememberMeDurationDays: s.rememberMeDurationDays?.toString() ?? '',
    thumbnailSize: s.thumbnailSize ?? 'DEFAULT',
    taskPoolSize: s.taskPoolSize?.toString() ?? '',
    koboProxy: s.koboProxy ?? false,
    skipReadOnNext: s.skipReadOnNext ?? false,
    recentReadingDays: String(s.recentReadingDays ?? 0),
  }
}

function positiveInt(v: string): boolean {
  return /^\d+$/.test(v) && Number(v) > 0
}

export function validateDraft(d: SettingsDraft): Partial<Record<keyof SettingsDraft, string>> {
  const errors: Partial<Record<keyof SettingsDraft, string>> = {}
  if (!positiveInt(d.rememberMeDurationDays.trim()))
    errors.rememberMeDurationDays = i18n.t('admin-settings:validation.positiveDays')
  if (!positiveInt(d.taskPoolSize.trim())) errors.taskPoolSize = i18n.t('admin-settings:validation.positiveInt')
  return errors
}

/** PATCH semantics: only dirty fields are sent. */
export function computeChanges(s: SettingsDto, d: SettingsDraft): SettingsUpdateDto {
  const changes: SettingsUpdateDto = {}
  if (d.deleteEmptyCollections !== (s.deleteEmptyCollections ?? false)) changes.deleteEmptyCollections = d.deleteEmptyCollections
  if (d.deleteEmptyReadLists !== (s.deleteEmptyReadLists ?? false)) changes.deleteEmptyReadLists = d.deleteEmptyReadLists
  if (d.rememberMeDurationDays.trim() !== (s.rememberMeDurationDays?.toString() ?? ''))
    changes.rememberMeDurationDays = Number(d.rememberMeDurationDays)
  if (d.thumbnailSize !== (s.thumbnailSize ?? 'DEFAULT')) changes.thumbnailSize = d.thumbnailSize
  if (d.taskPoolSize.trim() !== (s.taskPoolSize?.toString() ?? '')) changes.taskPoolSize = Number(d.taskPoolSize)
  if (d.koboProxy !== (s.koboProxy ?? false)) changes.koboProxy = d.koboProxy
  if (d.skipReadOnNext !== (s.skipReadOnNext ?? false)) changes.skipReadOnNext = d.skipReadOnNext
  if (d.recentReadingDays !== String(s.recentReadingDays ?? 0)) changes.recentReadingDays = Number(d.recentReadingDays)
  return changes
}

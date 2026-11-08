import { useQuery } from '@tanstack/react-query'
import { settingsApi } from '@/lib/api/settings'

/** server settings as the current user may read them (non-admins get the public subset) */
export const SERVER_SETTINGS_KEY = ['settings'] as const

/**
 * The server's recent reading window in days; 0 = off. On Deck is filtered by the server; Keep
 * Reading goes through the generic book search, so the client adds a `readDate` condition to it.
 */
export function useRecentReadingDays(): { days: number; isPending: boolean } {
  const query = useQuery({ queryKey: SERVER_SETTINGS_KEY, queryFn: settingsApi.get, staleTime: 5 * 60_000 })
  return { days: query.data?.recentReadingDays ?? 0, isPending: query.isPending }
}

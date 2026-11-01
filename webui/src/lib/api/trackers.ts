import { api } from './client'
import type {
  KomfOAuthProvider,
  KomfOAuthStatus,
  TrackerLink,
  TrackerLinkPage,
  TrackerLinkUpsert,
  TrackerPreferences,
  TrackerSearchItem,
  TrackerState,
  TrackerUpdatePayload,
} from './types'

/** TanStack Query keys for the tracker bindings; shared by every consumer. */
export const trackerLinkKeys = {
  all: ['trackers', 'links'] as const,
  list: (page: number, provider?: KomfOAuthProvider) => ['trackers', 'links', { page, provider }] as const,
  series: (seriesId: string) => ['trackers', 'links', seriesId] as const,
  preferences: ['trackers', 'preferences'] as const,
}

/** Platform entry page URL. */
export function trackerEntryUrl(provider: KomfOAuthProvider, trackId: string): string {
  switch (provider) {
    case 'anilist':
      return `https://anilist.co/manga/${trackId}`
    case 'mal':
      return `https://myanimelist.net/manga/${trackId}`
    case 'bangumi':
      return `https://bgm.tv/subject/${trackId}`
    case 'mangabaka':
      return `https://mangabaka.org/${trackId}`
  }
}

const TRACKER_PROVIDERS: KomfOAuthProvider[] = ['anilist', 'mal', 'bangumi', 'mangabaka']

function hostMatches(hostname: string, domain: string): boolean {
  return hostname === domain || hostname.endsWith(`.${domain}`)
}

function trackerEntryId(provider: KomfOAuthProvider, input: string): string | undefined {
  let url: URL
  try {
    url = new URL(input)
  } catch {
    return undefined
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return undefined

  switch (provider) {
    case 'anilist':
      if (hostMatches(url.hostname, 'anilist.co')) {
        return url.pathname.match(/^\/manga\/(\d+)(?:\/|$)/)?.[1]
      }
      return undefined
    case 'mal':
      if (hostMatches(url.hostname, 'myanimelist.net')) {
        return url.pathname.match(/^\/manga\/(\d+)(?:\/|$)/)?.[1]
      }
      return undefined
    case 'bangumi':
      if (hostMatches(url.hostname, 'bgm.tv') || hostMatches(url.hostname, 'bangumi.tv')) {
        return url.pathname.match(/^\/subject\/(\d+)(?:\/|$)/)?.[1]
      }
      return undefined
    case 'mangabaka':
      if (hostMatches(url.hostname, 'mangabaka.org')) {
        return url.pathname.match(/^\/(?:series\/)?(\d+)(?:\/|$)/)?.[1]
      }
      return undefined
  }
}

/** Finds platform entry links in series metadata links;
 * the returned URL is fed to the tracker search box, which resolves links directly. */
export function trackerUrlsFromMetadata(links: Array<{ url: string }>): Partial<Record<KomfOAuthProvider, string>> {
  const out: Partial<Record<KomfOAuthProvider, string>> = {}
  for (const { url } of links) {
    for (const provider of TRACKER_PROVIDERS) {
      if (out[provider]) continue
      const id = trackerEntryId(provider, url)
      if (id) out[provider] = trackerEntryUrl(provider, id)
    }
  }
  return out
}

export const trackersApi = {
  // bindings are kmrs-side state, scoped to the calling user
  listLinks: (params?: { page?: number; size?: number; provider?: KomfOAuthProvider }) =>
    api.get<TrackerLinkPage>('/api/v1/trackers/links', params),
  listSeriesLinks: (seriesId: string) => api.get<TrackerLink[]>(`/api/v1/trackers/links/${seriesId}`),
  upsertLink: (seriesId: string, body: TrackerLinkUpsert) =>
    api.put<TrackerLink>(`/api/v1/trackers/links/${seriesId}`, body),
  deleteLink: (seriesId: string, provider: KomfOAuthProvider) =>
    api.delete<void>(`/api/v1/trackers/links/${seriesId}/${provider}`),

  // per-user display preferences (kmrs-side state)
  getPreferences: () => api.get<TrackerPreferences>('/api/v1/trackers/preferences'),
  putPreferences: (body: TrackerPreferences) =>
    api.put<TrackerPreferences>('/api/v1/trackers/preferences', body),

  // native provider calls, scoped by the server to the calling user's tracker account
  search: (provider: KomfOAuthProvider, params: { name: string; nsfw?: boolean; language?: string }) =>
    api.get<TrackerSearchItem[]>(`/api/v1/trackers/${provider}/search`, params),
  getState: (provider: KomfOAuthProvider, trackId: string) =>
    api.get<TrackerState>(`/api/v1/trackers/${provider}/state`, { trackId }),
  update: (provider: KomfOAuthProvider, body: TrackerUpdatePayload) =>
    api.post<void>(`/api/v1/trackers/${provider}/update`, body),

  oauthStatus: (provider: KomfOAuthProvider) =>
    api.get<KomfOAuthStatus>(`/api/v1/trackers/oauth/${provider}/status`),
  oauthLogout: (provider: KomfOAuthProvider) =>
    api.post<void>(`/api/v1/trackers/oauth/${provider}/logout`),
  // start 302s to the provider's authorize page: a browser-navigation endpoint, never fetch it
  oauthStartUrl: (provider: KomfOAuthProvider) => `/api/v1/trackers/oauth/${provider}/start`,
}

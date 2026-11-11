import { api } from './client'

export interface CatalogNode {
  slug: string
  parentSlug: string | null
  name: string
  sortOrder: number
  boundLibraryName: string
  note: string
  seriesCount: number
  directory: string
  children: CatalogNode[]
}

export interface CatalogSeries {
  id: string
  name: string
  bookCount: number
  libraryId: string
  libraryName: string
}

export interface CatalogPage {
  category: CatalogNode
  ancestors: CatalogNode[]
  content: CatalogSeries[]
  totalElements: number
  number: number
  size: number
  last: boolean
}

export const catalogApi = {
  list: () => api.get<CatalogNode[]>('/api/v1/catalog'),
  get: (slug: string, page = 0, size = 48, directory = '') =>
    api.get<CatalogPage>(`/api/v1/catalog/${slug}`, {
      page,
      size,
      directory: directory || undefined,
    }),
}

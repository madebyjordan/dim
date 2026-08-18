import type {
  Media,
  MediaFile,
  MediaSummary,
  SearchResult
} from '$lib/api/generated';

export type CatalogItem = MediaSummary & {
  library_id?: number;
  season?: number;
  episode?: number;
  media_type?: string;
};

export type ShowPresentation = {
  seasonCount?: number;
  startYear?: number;
  endYear?: number;
  ongoing: boolean;
};

export function showYearLabel(show: ShowPresentation): string | null {
  if (!show.startYear) return null;
  if (show.ongoing) return `${show.startYear}–Present`;
  if (show.endYear && show.endYear !== show.startYear) {
    return `${show.startYear}–${show.endYear}`;
  }
  return String(show.startYear);
}

export function seasonCountLabel(count: number): string {
  return `${count} ${count === 1 ? 'Season' : 'Seasons'}`;
}

export function flattenLibraryMedia(
  groups: Record<string, Array<MediaSummary>>
): Array<CatalogItem> {
  return Object.values(groups).flat();
}

export function fromSearchResult(result: SearchResult): CatalogItem {
  return result;
}

export function imageUrl(path: string | null | undefined): string | null {
  if (!path) return null;
  if (/^(?:https?:)?\/\//.test(path)) return path;
  return `/${path.replace(/^\//, '')}`;
}

export function runtimeLabel(seconds: number): string | null {
  if (!Number.isFinite(seconds) || seconds <= 0) return null;
  const minutes = Math.round(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const remainder = minutes % 60;
  if (hours === 0) return `${minutes}m`;
  return remainder === 0 ? `${hours}h` : `${hours}h ${remainder}m`;
}

export function playableMediaId(media: Media): number {
  return media.play_btn_id ?? media.id;
}

export function playableFile(
  media: Media,
  files: Array<MediaFile>
): MediaFile | null {
  const mediaId = playableMediaId(media);
  return files.find((file) => file.media_id === mediaId) ?? files[0] ?? null;
}

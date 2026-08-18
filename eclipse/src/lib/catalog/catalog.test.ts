import { describe, expect, it } from 'vitest';
import {
  flattenLibraryMedia,
  imageUrl,
  playableFile,
  runtimeLabel,
  seasonCountLabel,
  showYearLabel
} from './catalog';

describe('catalog presentation helpers', () => {
  it('flattens the backend library envelope without inventing sections', () => {
    expect(
      flattenLibraryMedia({ Movies: [{ id: 1, name: 'Arrival' }] })
    ).toEqual([{ id: 1, name: 'Arrival' }]);
  });

  it('normalizes stored image paths and preserves remote URLs', () => {
    expect(imageUrl('images/poster.jpg')).toBe('/images/poster.jpg');
    expect(imageUrl('/images/poster.jpg')).toBe('/images/poster.jpg');
    expect(imageUrl('https://image.test/poster.jpg')).toBe(
      'https://image.test/poster.jpg'
    );
  });

  it('formats real durations without placeholder values', () => {
    expect(runtimeLabel(7_560)).toBe('2h 6m');
    expect(runtimeLabel(0)).toBeNull();
  });

  it('chooses the playable episode file for a show', () => {
    const files = [
      { id: 10, media_id: 4 },
      { id: 11, media_id: 5 }
    ];
    expect(
      playableFile(
        { id: 1, play_btn_id: 5 } as Parameters<typeof playableFile>[0],
        files as Parameters<typeof playableFile>[1]
      )?.id
    ).toBe(11);
  });

  it('formats show runs and season counts', () => {
    expect(
      showYearLabel({
        seasonCount: 5,
        startYear: 2008,
        endYear: 2013,
        ongoing: false
      })
    ).toBe('2008–2013');
    expect(
      showYearLabel({ seasonCount: 2, startYear: 2024, ongoing: true })
    ).toBe('2024–Present');
    expect(
      showYearLabel({ seasonCount: 1, startYear: 2008, ongoing: false })
    ).toBe('2008');
    expect(seasonCountLabel(1)).toBe('1 Season');
    expect(seasonCountLabel(5)).toBe('5 Seasons');
  });
});

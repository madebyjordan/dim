// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';
import { session } from '$lib/auth/session.svelte';
import EditMediaDialog from './EditMediaDialog.svelte';

const components: Record<string, any>[] = [];
const library = {
  id: 2,
  name: 'Movies',
  locations: [],
  media_type: 'movie',
  auto_scan: true
};
const media = {
  id: 9,
  library_id: 2,
  name: 'City of God',
  media_type: 'movie',
  genres: [],
  duration: 0
};
const file = {
  id: 14,
  media_id: 9,
  library_id: 2,
  target_file: '[scloudx.lol] 021.City.of.God.2002.mkv',
  raw_name: 'City of God',
  manual_override: false,
  metadata_provider: 'local',
  match_provenance: 'local_filename'
};

beforeEach(() => {
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
    configurable: true,
    value() {
      this.open = true;
    }
  });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', {
    configurable: true,
    value() {
      this.open = false;
    }
  });
});

afterEach(async () => {
  vi.restoreAllMocks();
  await Promise.all(
    components.splice(0).map((component) => unmount(component))
  );
  document.body.innerHTML = '';
});

async function settle() {
  await Promise.resolve();
  await tick();
  await Promise.resolve();
  await tick();
}

function render(onsaved = vi.fn(), onclose = vi.fn()) {
  components.push(
    mount(EditMediaDialog, {
      target: document.body,
      props: { open: true, media, file, library, onsaved, onclose }
    })
  );
  return { onsaved, onclose };
}

function input(label: string, value: string) {
  const field = Array.from(document.querySelectorAll('label'))
    .find((candidate) => candidate.textContent?.startsWith(label))
    ?.querySelector('input, textarea') as HTMLInputElement;
  field.value = value;
  field.dispatchEvent(new Event('input', { bubbles: true }));
}

describe('EditMediaDialog', () => {
  it('shows unmatched file context and immediately applies an automatic match', async () => {
    vi.spyOn(session.api, 'get').mockImplementation((async (path: string) =>
      path === 'media/9/files'
        ? [file, { ...file, id: 15, target_file: 'City.of.God.alt.mkv' }]
        : [
            {
              id: '598',
              title: 'City of God',
              year: 2002,
              overview: 'Two boys take different paths through Rio de Janeiro.',
              poster_path: 'https://image.example/city.jpg'
            }
          ]) as typeof session.api.get);
    const patch = vi
      .spyOn(session.api, 'patch')
      .mockResolvedValue(undefined as never);
    const callbacks = render();
    await settle();

    expect(document.body.textContent).toContain('Unmatched');
    expect(document.body.textContent).toContain('City of God (2002)');
    (
      Array.from(document.querySelectorAll('button')).find((button) =>
        button.textContent?.includes('City of God (2002)')
      ) as HTMLButtonElement
    ).click();
    await settle();

    expect(patch).toHaveBeenCalledWith('mediafile/match', {
      tmdb_id: '598',
      media_type: 'movie',
      mediafiles: [14, 15]
    });
    expect(callbacks.onclose).toHaveBeenCalledOnce();
    expect(callbacks.onsaved).toHaveBeenCalledWith('City of God');
  });

  it('saves all manual metadata fields through the persistent override endpoint', async () => {
    vi.spyOn(session.api, 'get').mockImplementation((async (path: string) =>
      path === 'media/9/files' ? [file] : []) as typeof session.api.get);
    const patch = vi
      .spyOn(session.api, 'patch')
      .mockResolvedValue(undefined as never);
    render();
    await settle();

    (
      Array.from(document.querySelectorAll('button')).find(
        (button) => button.textContent === 'Manual'
      ) as HTMLButtonElement
    ).click();
    await tick();
    input('Artwork URL', 'https://example.com/home-video.jpg');
    input('Title', 'Family Holiday');
    input('Synopsis', 'A private family recording.');
    input('Year', '2024');
    input('Rating', '8.5');
    input('Genres', 'Home Video, Family');
    input('Language', 'English');
    const save = Array.from(document.querySelectorAll('button')).find(
      (button) => button.textContent === 'Save'
    ) as HTMLButtonElement;
    expect(save.disabled).toBe(false);
    expect(
      (document.querySelector('form.manual') as HTMLFormElement).checkValidity()
    ).toBe(true);
    save.dispatchEvent(
      new MouseEvent('click', { bubbles: true, cancelable: true })
    );
    await settle();

    expect(patch).toHaveBeenCalledWith('media/9/manual', {
      title: 'Family Holiday',
      synopsis: 'A private family recording.',
      year: 2024,
      genres: ['Home Video', 'Family'],
      language: 'English',
      rating: 8.5,
      artwork: 'https://example.com/home-video.jpg'
    });
  });
});

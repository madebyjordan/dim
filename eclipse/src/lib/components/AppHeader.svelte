<script lang="ts">
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import type { Library, User } from '$lib/api/generated';
  import { imageUrl } from '$lib/catalog/catalog';
  import IconButton from '$lib/primitives/IconButton.svelte';
  import DropdownMenu from '$lib/primitives/DropdownMenu.svelte';
  import UserIcon from '$lib/icons/UserIcon.svelte';
  import EclipseMark from './EclipseMark.svelte';
  import LibrarySettingsMenu from './LibrarySettingsMenu.svelte';

  let {
    libraries,
    activeLibraryId,
    user,
    scanText,
    activeLibraryScanning,
    onlibraryautoscan,
    onlibraryscan,
    onlibrarydelete,
    onlibrary,
    onsearch,
    onsearchclose,
    onaddlibrary,
    onlogout
  }: {
    libraries: Array<Library>;
    activeLibraryId: number | null;
    user: User | null;
    scanText: string | null;
    activeLibraryScanning: boolean;
    onlibraryautoscan: (
      library: Library,
      enabled: boolean
    ) => void | Promise<void>;
    onlibraryscan: (library: Library) => void | Promise<void>;
    onlibrarydelete: (library: Library) => void | Promise<void>;
    onlibrary: (library: Library) => void;
    onsearch: (query: string) => void;
    onsearchclose: () => void;
    onaddlibrary: () => void;
    onlogout: () => void;
  } = $props();

  let searchOpen = $state(false);
  let searchQuery = $state('');
  let searchInput = $state<HTMLInputElement>();
  const activeLibrary = $derived(
    libraries.find((library) => library.id === activeLibraryId) ?? null
  );

  $effect(() => {
    const query = searchQuery.trim();
    const timer = window.setTimeout(() => {
      if (searchOpen) onsearch(query);
    }, 220);
    return () => window.clearTimeout(timer);
  });

  function openSearch() {
    searchOpen = true;
    window.setTimeout(() => searchInput?.focus());
  }

  function closeSearch() {
    searchOpen = false;
    searchQuery = '';
    onsearchclose();
  }
</script>

{#snippet profileTrigger(attributes: HTMLButtonAttributes)}
  <IconButton {...attributes} label="Open profile menu" tone="surface">
    {#if user?.picture}
      <img class="profile-avatar" src={imageUrl(user.picture) ?? ''} alt="" />
    {:else}
      <UserIcon size="100%" />
    {/if}
  </IconButton>
{/snippet}

{#snippet profileHeader()}
  <span class="profile-name">{user?.username}</span>
{/snippet}

<header class="header">
  <div class="leading">
    <div class:open={searchOpen} class="search">
      <IconButton
        label={searchOpen ? 'Close search' : 'Search media'}
        onclick={() => (searchOpen ? closeSearch() : openSearch())}
      >
        {#if searchOpen}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="m6 6 12 12M18 6 6 18" />
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="10.8" cy="10.8" r="7.2" />
            <path d="m16.2 16.2 5 5" />
          </svg>
        {/if}
      </IconButton>
      {#if searchOpen}
        <input
          bind:this={searchInput}
          bind:value={searchQuery}
          type="search"
          aria-label="Search media"
          placeholder="Search"
          onkeydown={(event) => {
            if (event.key === 'Escape') closeSearch();
          }}
        />
      {/if}
    </div>

    <nav aria-label="Libraries">
      {#each libraries as library (library.id)}
        <button
          type="button"
          class:active={activeLibraryId === library.id && !searchOpen}
          onclick={() => onlibrary(library)}>{library.name}</button
        >
      {/each}
      {#if user?.roles.includes('owner')}
        <button type="button" class="add" onclick={onaddlibrary}
          >Add Library</button
        >
      {/if}
    </nav>
  </div>

  <a class="brand" href="/" aria-label="Eclipse home"><EclipseMark /></a>

  <div class="trailing">
    {#if scanText}
      <div class="scan" role="status" aria-live="polite">
        <span>{scanText}</span><i aria-hidden="true"></i>
      </div>
    {/if}
    {#if user?.roles.includes('owner') && activeLibrary}
      <LibrarySettingsMenu
        library={activeLibrary}
        scanning={activeLibraryScanning}
        onautoscan={(enabled) => onlibraryautoscan(activeLibrary, enabled)}
        onscan={() => onlibraryscan(activeLibrary)}
        ondelete={() => onlibrarydelete(activeLibrary)}
      />
    {/if}
    <DropdownMenu
      label="Profile menu"
      trigger={profileTrigger}
      header={profileHeader}
      align="end"
      items={[{ label: 'Sign out', onselect: onlogout }]}
    />
  </div>
</header>

<style>
  .header {
    position: relative;
    z-index: 20;
    min-height: var(--header-height);
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
    align-items: center;
    gap: var(--space-6);
    padding-inline: var(--layout-gutter);
  }
  .leading,
  .trailing,
  nav,
  .scan {
    display: flex;
    align-items: center;
  }
  .leading {
    min-width: 0;
    gap: var(--space-7);
  }
  .trailing {
    justify-content: end;
    gap: var(--space-5);
  }
  .brand {
    grid-column: 2;
  }
  .search {
    height: 52px;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .search.open {
    width: 280px;
  }
  nav button {
    border: 0;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }
  .search svg {
    width: 100%;
    height: 100%;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: 1.5;
  }
  input {
    min-width: 0;
    width: 100%;
    padding: 8px 0;
    border: 0;
    border-bottom: 1px solid rgba(255, 255, 255, 0.35);
    outline: 0;
    color: var(--color-fg);
    background: transparent;
    font-size: var(--text-md);
  }
  nav {
    min-width: 0;
    gap: var(--space-5);
    overflow-x: auto;
    scrollbar-width: none;
  }
  nav::-webkit-scrollbar {
    display: none;
  }
  nav button {
    flex: none;
    padding: 9px 0;
    color: var(--color-fg-subtle);
    font-size: var(--text-lg);
    font-weight: 560;
    letter-spacing: -0.025em;
    white-space: nowrap;
    transition: color var(--motion-fast) ease;
  }
  nav button:hover,
  nav button.active,
  nav button:focus-visible {
    color: var(--color-fg);
  }
  .scan {
    gap: 10px;
    color: var(--color-fg-subtle);
    font-size: var(--text-sm);
    white-space: nowrap;
  }
  .scan i {
    width: 13px;
    aspect-ratio: 1;
    border: 1.5px solid rgba(255, 255, 255, 0.22);
    border-top-color: var(--color-fg);
    border-radius: 50%;
    animation: spin 900ms linear infinite;
  }
  .profile-avatar {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .profile-name {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (max-width: 1100px) {
    .header {
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 16px;
    }
    .leading {
      gap: 18px;
    }
    .brand {
      display: none;
    }
    .scan {
      display: none;
    }
    nav {
      gap: 18px;
    }
  }
  @media (max-width: 620px) {
    .search.open {
      width: 100%;
    }
    .search.open + nav {
      display: none;
    }
    nav button {
      font-size: 15px;
    }
  }
</style>

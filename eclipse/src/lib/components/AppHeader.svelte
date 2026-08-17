<script lang="ts">
  import { onMount } from 'svelte';
  import type { Library, User } from '$lib/api/generated';
  import { imageUrl } from '$lib/catalog/catalog';
  import IconButton from '$lib/primitives/IconButton.svelte';
  import EclipseMark from './EclipseMark.svelte';

  let {
    libraries,
    activeLibraryId,
    user,
    scanText,
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
    onlibrary: (library: Library) => void;
    onsearch: (query: string) => void;
    onsearchclose: () => void;
    onaddlibrary: () => void;
    onlogout: () => void;
  } = $props();

  let searchOpen = $state(false);
  let searchQuery = $state('');
  let profileOpen = $state(false);
  let searchInput = $state<HTMLInputElement>();
  let profile = $state<HTMLDivElement>();

  $effect(() => {
    const query = searchQuery.trim();
    const timer = window.setTimeout(() => {
      if (searchOpen) onsearch(query);
    }, 220);
    return () => window.clearTimeout(timer);
  });

  onMount(() => {
    const closeProfile = (event: PointerEvent) => {
      if (profileOpen && profile && !profile.contains(event.target as Node)) {
        profileOpen = false;
      }
    };
    window.addEventListener('pointerdown', closeProfile);
    return () => window.removeEventListener('pointerdown', closeProfile);
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
    <div class="profile" bind:this={profile}>
      <IconButton
        label="Open profile menu"
        appearance="surface"
        expanded={profileOpen}
        onclick={() => (profileOpen = !profileOpen)}
      >
        {#if user?.picture}
          <img
            class="profile-avatar"
            src={imageUrl(user.picture) ?? ''}
            alt=""
          />
        {:else}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="8" r="3.1" />
            <path d="M5.5 20v-1.2a6.5 6.5 0 0 1 13 0V20" />
          </svg>
        {/if}
      </IconButton>
      {#if profileOpen}
        <div class="profile-menu">
          <p>{user?.username}</p>
          <button type="button" onclick={onlogout}>Sign out</button>
        </div>
      {/if}
    </div>
  </div>
</header>

<style>
  .header {
    position: relative;
    z-index: 20;
    min-height: clamp(92px, 10.2vh, 180px);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 28px;
    padding: 0 clamp(28px, 4.9vw, 280px);
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
    gap: clamp(30px, 3.5vw, 190px);
  }
  .trailing {
    justify-content: end;
    gap: clamp(18px, 2vw, 80px);
  }
  .brand {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
  }
  .search {
    height: 52px;
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .search.open {
    width: clamp(190px, 19vw, 440px);
  }
  nav button,
  .profile-menu button {
    border: 0;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }
  .search svg,
  .profile svg {
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
    font-size: clamp(17px, 1.15vw, 34px);
  }
  nav {
    min-width: 0;
    gap: clamp(18px, 2.3vw, 110px);
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
    font-size: clamp(17px, 1.25vw, 38px);
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
    font-size: clamp(12px, 0.78vw, 22px);
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
  .profile {
    position: relative;
  }
  .profile-avatar {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .profile-menu {
    position: absolute;
    top: calc(100% + 12px);
    right: 0;
    min-width: 170px;
    padding: 12px;
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-surface) 96%, transparent);
    box-shadow: var(--shadow-float);
  }
  .profile-menu p {
    margin: var(--space-1) var(--space-2) 10px;
    overflow: hidden;
    color: var(--color-fg-subtle);
    font-size: 13px;
    text-overflow: ellipsis;
  }
  .profile-menu button {
    width: 100%;
    padding: 9px 8px;
    border-radius: 7px;
    text-align: left;
  }
  .profile-menu button:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (max-width: 900px) {
    .header {
      min-height: 78px;
      gap: 16px;
      padding-inline: 20px;
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
      position: absolute;
      inset: 13px 76px 13px 18px;
      z-index: 3;
      width: auto;
      padding-right: 8px;
      background: var(--color-canvas);
    }
    .search.open + nav {
      visibility: hidden;
    }
    nav button {
      font-size: 15px;
    }
  }
</style>

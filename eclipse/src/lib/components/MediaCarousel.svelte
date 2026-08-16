<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { CatalogItem } from '$lib/catalog/catalog';
  import { imageUrl } from '$lib/catalog/catalog';
  import LazyPoster from './LazyPoster.svelte';

  let {
    items,
    selectedId,
    playable,
    onselect,
    onplay
  }: {
    items: Array<CatalogItem>;
    selectedId: number | null;
    playable: boolean;
    onselect: (item: CatalogItem) => void;
    onplay: () => void;
  } = $props();

  let rail: HTMLDivElement;

  function focusCard(index: number) {
    const bounded = Math.max(0, Math.min(items.length - 1, index));
    const card = rail.querySelector<HTMLButtonElement>(
      `[data-media-index="${bounded}"]`
    );
    card?.focus();
    card?.scrollIntoView({
      behavior: 'smooth',
      block: 'nearest',
      inline: 'center'
    });
  }

  function handleKey(event: KeyboardEvent, index: number) {
    if (event.key === 'ArrowRight') {
      event.preventDefault();
      focusCard(index + 1);
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      focusCard(index - 1);
    } else if (event.key === 'Home') {
      event.preventDefault();
      focusCard(0);
    } else if (event.key === 'End') {
      event.preventDefault();
      focusCard(items.length - 1);
    }
  }

  onMount(() => {
    void tick().then(() => {
      if (window.matchMedia('(min-width: 720px)').matches) {
        const card = rail.querySelector<HTMLElement>('.card');
        if (card) rail.scrollLeft = card.offsetWidth * 0.62;
      }
    });
  });
</script>

<div
  class="carousel"
  bind:this={rail}
  role="region"
  aria-label="Media library"
  tabindex="-1"
>
  <div class="track">
    {#each items as item, index (item.id)}
      <article
        class:selected={selectedId === item.id}
        class="card"
        aria-label={item.name}
      >
        <button
          type="button"
          class="select"
          data-media-index={index}
          aria-pressed={selectedId === item.id}
          aria-label={`Select ${item.name}`}
          onclick={() => onselect(item)}
          onkeydown={(event) => handleKey(event, index)}
        >
          <LazyPoster src={imageUrl(item.poster_path)} alt={item.name} />
        </button>
        {#if selectedId === item.id && playable}
          <button
            type="button"
            class="play"
            aria-label={`Play ${item.name}`}
            onclick={onplay}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M8 5.5v13l10-6.5z" />
            </svg>
          </button>
        {/if}
      </article>
    {/each}
  </div>
</div>

<style>
  .carousel {
    width: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    overscroll-behavior-inline: contain;
    scrollbar-width: none;
    scroll-behavior: smooth;
    scroll-snap-type: x proximity;
  }
  .carousel::-webkit-scrollbar {
    display: none;
  }
  .track {
    width: max-content;
    display: flex;
    align-items: end;
    gap: clamp(18px, 2.6vw, 150px);
    padding: 18px clamp(32px, 5.2vw, 300px) clamp(28px, 4.5vh, 110px);
  }
  .card {
    position: relative;
    width: clamp(150px, 13.5vw, 780px);
    aspect-ratio: 2 / 3;
    flex: none;
    overflow: hidden;
    border-radius: clamp(14px, 1vw, 38px);
    background: #141414;
    content-visibility: auto;
    contain-intrinsic-size: auto 300px 450px;
    scroll-snap-align: center;
    transition:
      transform 220ms ease,
      box-shadow 220ms ease;
  }
  .card.selected {
    transform: translateY(-12px) scale(1.025);
    box-shadow:
      0 0 0 3px rgba(255, 255, 255, 0.92),
      0 20px 55px rgba(0, 0, 0, 0.56);
  }
  .select {
    width: 100%;
    height: 100%;
    display: block;
    padding: 0;
    overflow: hidden;
    border: 0;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }
  .select:focus-visible {
    outline: 3px solid #fff;
    outline-offset: -6px;
  }
  .play {
    position: absolute;
    inset: 50% auto auto 50%;
    width: clamp(48px, 4vw, 104px);
    aspect-ratio: 1;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid rgba(255, 255, 255, 0.7);
    border-radius: 50%;
    color: #0a0a0a;
    background: rgba(255, 255, 255, 0.9);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.46);
    cursor: pointer;
    transform: translate(-50%, -50%);
  }
  .play svg {
    width: 45%;
    fill: currentColor;
    transform: translateX(5%);
  }
  @media (max-width: 700px) {
    .track {
      gap: 16px;
      padding-inline: 18px;
    }
    .card {
      width: min(42vw, 190px);
    }
    .card.selected {
      transform: translateY(-7px) scale(1.015);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .carousel {
      scroll-behavior: auto;
    }
    .card {
      transition: none;
    }
  }
</style>

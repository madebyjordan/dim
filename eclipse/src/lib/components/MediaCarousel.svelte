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
          {#if item.episode !== undefined}
            <span class="episode-label">
              <strong
                >S{String(item.season ?? 0).padStart(2, '0')} E{String(
                  item.episode
                ).padStart(2, '0')}</strong
              >
              <span>{item.name}</span>
            </span>
          {/if}
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
    gap: var(--space-6);
    padding: calc(var(--space-4) + 12px) var(--layout-gutter) var(--space-6);
  }
  .card {
    position: relative;
    width: 190px;
    aspect-ratio: 2 / 3;
    flex: none;
    overflow: visible;
    border-radius: 20px;
    background: var(--color-surface);
    isolation: isolate;
    scroll-snap-align: center;
    transition:
      transform var(--motion-normal) ease,
      box-shadow var(--motion-normal) ease;
  }
  .card.selected {
    transform: translateY(-12px) scale(1.025);
    box-shadow: 0 20px 55px rgba(0, 0, 0, 0.56);
  }
  .card.selected::before {
    position: absolute;
    z-index: 3;
    inset: -3px;
    padding: 3px;
    border-radius: inherit;
    background: conic-gradient(
      from var(--selected-border-angle),
      rgba(255, 255, 255, 0) 0deg,
      rgba(255, 255, 255, var(--selected-glow-one)) var(--selected-stop-one),
      rgba(255, 255, 255, 0) calc(var(--selected-stop-one) + 58deg),
      rgba(255, 255, 255, var(--selected-glow-two)) var(--selected-stop-two),
      rgba(255, 255, 255, 0) calc(var(--selected-stop-two) + 66deg),
      rgba(255, 255, 255, var(--selected-glow-three)) var(--selected-stop-three),
      rgba(255, 255, 255, 0) 360deg
    );
    -webkit-mask:
      linear-gradient(#fff 0 0) content-box,
      linear-gradient(#fff 0 0);
    mask:
      linear-gradient(#fff 0 0) content-box,
      linear-gradient(#fff 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    pointer-events: none;
    content: '';
    animation: selected-border-shimmer 18s ease-in-out infinite;
  }
  .select {
    position: relative;
    z-index: 1;
    width: 100%;
    height: 100%;
    display: block;
    padding: 0;
    overflow: hidden;
    border: 0;
    border-radius: inherit;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }
  .select:focus-visible {
    outline: 3px solid var(--color-fg);
    outline-offset: -6px;
  }
  .episode-label {
    position: absolute;
    inset: auto 0 0;
    display: grid;
    gap: 3px;
    padding: 42px 14px 14px;
    color: var(--color-fg);
    background: linear-gradient(transparent, rgba(0, 0, 0, 0.92));
    text-align: left;
  }
  .episode-label strong {
    font-size: var(--text-xs);
    letter-spacing: 0.05em;
  }
  .episode-label span {
    overflow: hidden;
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .play {
    position: absolute;
    z-index: 2;
    inset: 50% auto auto 50%;
    width: 64px;
    aspect-ratio: 1;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid rgba(255, 255, 255, 0.7);
    border-radius: var(--radius-round);
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
      width: 160px;
      border-radius: var(--radius-md);
    }
    .card.selected {
      transform: translateY(-7px) scale(1.015);
    }
  }
  @media (min-width: 1800px) {
    .track {
      gap: var(--space-7);
    }
    .card {
      width: 240px;
      border-radius: var(--radius-lg);
    }
    .play {
      width: 76px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .carousel {
      scroll-behavior: auto;
    }
    .card {
      transition: none;
    }
    .card.selected::before {
      animation: none;
    }
  }

  @property --selected-border-angle {
    syntax: '<angle>';
    inherits: false;
    initial-value: 0deg;
  }
  @property --selected-stop-one {
    syntax: '<angle>';
    inherits: false;
    initial-value: 34deg;
  }
  @property --selected-stop-two {
    syntax: '<angle>';
    inherits: false;
    initial-value: 174deg;
  }
  @property --selected-stop-three {
    syntax: '<angle>';
    inherits: false;
    initial-value: 292deg;
  }
  @property --selected-glow-one {
    syntax: '<number>';
    inherits: false;
    initial-value: 1;
  }
  @property --selected-glow-two {
    syntax: '<number>';
    inherits: false;
    initial-value: 0.28;
  }
  @property --selected-glow-three {
    syntax: '<number>';
    inherits: false;
    initial-value: 0.64;
  }
  @keyframes selected-border-shimmer {
    0% {
      --selected-border-angle: 7deg;
      --selected-stop-one: 34deg;
      --selected-stop-two: 174deg;
      --selected-stop-three: 292deg;
      --selected-glow-one: 1;
      --selected-glow-two: 0.22;
      --selected-glow-three: 0.62;
      opacity: 0.72;
    }
    17% {
      --selected-border-angle: 81deg;
      --selected-stop-one: 61deg;
      --selected-stop-two: 203deg;
      --selected-stop-three: 274deg;
      --selected-glow-one: 0.3;
      --selected-glow-two: 1;
      --selected-glow-three: 0.12;
      opacity: 0.94;
    }
    36% {
      --selected-border-angle: 49deg;
      --selected-stop-one: 22deg;
      --selected-stop-two: 158deg;
      --selected-stop-three: 327deg;
      --selected-glow-one: 0.76;
      --selected-glow-two: 0.16;
      --selected-glow-three: 1;
      opacity: 0.64;
    }
    58% {
      --selected-border-angle: 191deg;
      --selected-stop-one: 79deg;
      --selected-stop-two: 226deg;
      --selected-stop-three: 301deg;
      --selected-glow-one: 0.12;
      --selected-glow-two: 0.7;
      --selected-glow-three: 0.34;
      opacity: 1;
    }
    77% {
      --selected-border-angle: 154deg;
      --selected-stop-one: 43deg;
      --selected-stop-two: 181deg;
      --selected-stop-three: 342deg;
      --selected-glow-one: 0.9;
      --selected-glow-two: 0.08;
      --selected-glow-three: 0.82;
      opacity: 0.7;
    }
    100% {
      --selected-border-angle: 367deg;
      --selected-stop-one: 34deg;
      --selected-stop-two: 174deg;
      --selected-stop-three: 292deg;
      --selected-glow-one: 1;
      --selected-glow-two: 0.22;
      --selected-glow-three: 0.62;
      opacity: 0.72;
    }
  }
</style>

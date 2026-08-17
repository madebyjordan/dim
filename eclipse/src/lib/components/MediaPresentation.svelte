<script lang="ts">
  import type {
    Media,
    PlaybackSession,
    PlaybackTrack
  } from '$lib/api/generated';
  import { runtimeLabel } from '$lib/catalog/catalog';
  import Select from '$lib/primitives/Select.svelte';

  let {
    media,
    playback,
    playbackLoading,
    playbackError,
    selectedVideo,
    selectedAudio,
    selectedSubtitle,
    onvideo,
    onaudio,
    onsubtitle
  }: {
    media: Media;
    playback: PlaybackSession | null;
    playbackLoading: boolean;
    playbackError: string | null;
    selectedVideo: string;
    selectedAudio: string;
    selectedSubtitle: string;
    onvideo: (id: string) => void;
    onaudio: (id: string) => void;
    onsubtitle: (id: string) => void;
  } = $props();

  let synopsisExpanded = $state(false);
  const tracks = (kind: PlaybackTrack['content_type']) =>
    playback?.tracks.filter((track) => track.content_type === kind) ?? [];
  const runtime = $derived(runtimeLabel(media.duration));
</script>

<section class:expanded={synopsisExpanded} class="presentation">
  <div class="content">
    <h1>{media.name}</h1>
    <div class="metadata" aria-label="Media details">
      {#if media.year}<span>{media.year}</span>{/if}
      {#each media.genres as genre}<span>{genre}</span>{/each}
      {#if runtime}<span>{runtime}</span>{/if}
    </div>

    {#if media.description}
      {#if synopsisExpanded}
        <div class="synopsis expanded-copy">
          <p>{media.description}</p>
          <button type="button" onclick={() => (synopsisExpanded = false)}
            >Show less</button
          >
        </div>
      {:else}
        <button
          type="button"
          class="synopsis collapsed-copy"
          aria-label="Expand synopsis"
          onclick={() => (synopsisExpanded = true)}
        >
          <span>{media.description}</span>
        </button>
      {/if}
    {/if}

    {#if !synopsisExpanded}
      <div class="selectors" aria-label="Playback options">
        {#if playbackLoading}
          <span class="preparing">Preparing playback…</span>
        {:else if playback}
          <Select
            label="Video quality"
            value={selectedVideo}
            onchange={(event) => onvideo(event.currentTarget.value)}
          >
            {#each tracks('video') as track}
              <option value={track.id}
                >{track.label || track.height || track.id}</option
              >
            {/each}
          </Select>
          <Select
            label="Audio track"
            value={selectedAudio}
            onchange={(event) => onaudio(event.currentTarget.value)}
          >
            {#each tracks('audio') as track}
              <option value={track.id}
                >{track.label || track.lang || track.id}</option
              >
            {/each}
          </Select>
          <Select
            label="Subtitle track"
            value={selectedSubtitle}
            onchange={(event) => onsubtitle(event.currentTarget.value)}
          >
            <option value="">No Subtitles</option>
            {#each tracks('subtitle') as track}
              <option value={track.id}
                >{track.label || track.lang || track.id}</option
              >
            {/each}
          </Select>
        {/if}
      </div>
      {#if playbackError}<p class="playback-error" role="status">
          {playbackError}
        </p>{/if}
    {/if}
  </div>
</section>

<style>
  .presentation {
    position: relative;
    z-index: 2;
    width: 100%;
    padding: var(--space-4) var(--layout-gutter) var(--space-5);
  }
  .content {
    width: min(100%, var(--content-width));
    display: grid;
    gap: var(--space-3);
    animation: content-in 320ms ease-out both;
  }
  h1,
  p {
    margin: 0;
  }
  h1 {
    max-width: 25ch;
    font-size: clamp(40px, 3vw, 96px);
    font-weight: 400;
    line-height: 1;
    text-wrap: balance;
  }
  .metadata {
    display: flex;
    flex-wrap: wrap;
    gap: 7px 18px;
    color: var(--color-fg);
    font-size: var(--text-md);
  }
  .synopsis {
    width: min(100%, 60ch);
    color: rgba(255, 255, 255, 0.78);
    font-size: var(--text-md);
    line-height: 1.55;
  }
  .collapsed-copy {
    position: relative;
    max-height: 7.75em;
    display: block;
    overflow: hidden;
    padding: 0 0 2.4em;
    border: 0;
    text-align: left;
    background: transparent;
    cursor: pointer;
    -webkit-mask-image: linear-gradient(#000 0 58%, transparent 100%);
    mask-image: linear-gradient(#000 0 58%, transparent 100%);
  }
  .expanded-copy {
    max-height: 20rem;
    overflow-y: auto;
    padding-right: 18px;
  }
  .expanded-copy button {
    margin-top: 18px;
    padding: 0;
    border: 0;
    color: var(--color-fg);
    background: transparent;
    font-weight: 650;
    cursor: pointer;
  }
  .selectors {
    min-height: 42px;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .preparing,
  .playback-error {
    color: var(--color-fg-subtle);
    font-size: var(--text-sm);
  }
  .playback-error {
    color: var(--color-danger);
  }
  @keyframes content-in {
    from {
      opacity: 0;
      transform: translateY(9px);
    }
  }
  @media (max-width: 900px) {
    h1 {
      font-size: clamp(34px, 9vw, 64px);
    }
  }
  @media (max-height: 720px) and (min-width: 901px) {
    h1 {
      font-size: clamp(34px, 4vw, 68px);
    }
    .collapsed-copy {
      max-height: 5.8em;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .content {
      animation: none;
    }
  }
</style>

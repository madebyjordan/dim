<script lang="ts">
  import type {
    Media,
    PlaybackSession,
    PlaybackTrack
  } from '$lib/api/generated';
  import { imageUrl, runtimeLabel } from '$lib/catalog/catalog';

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
  const backdrop = $derived(imageUrl(media.backdrop_path));
</script>

<section class:expanded={synopsisExpanded} class="presentation">
  {#if backdrop}
    <div
      class="backdrop"
      style:background-image={`linear-gradient(90deg, rgba(8, 8, 8, 0.94) 0%, rgba(8, 8, 8, 0.6) 43%, rgba(8, 8, 8, 0.12) 75%), linear-gradient(0deg, #0d0d0d 0%, transparent 52%), url("${backdrop}")`}
    ></div>
  {/if}
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
          <select
            aria-label="Video quality"
            value={selectedVideo}
            onchange={(event) => onvideo(event.currentTarget.value)}
          >
            {#each tracks('video') as track}
              <option value={track.id}
                >{track.label || track.height || track.id}</option
              >
            {/each}
          </select>
          <select
            aria-label="Audio track"
            value={selectedAudio}
            onchange={(event) => onaudio(event.currentTarget.value)}
          >
            {#each tracks('audio') as track}
              <option value={track.id}
                >{track.label || track.lang || track.id}</option
              >
            {/each}
          </select>
          <select
            aria-label="Subtitle track"
            value={selectedSubtitle}
            onchange={(event) => onsubtitle(event.currentTarget.value)}
          >
            <option value="">No Subtitles</option>
            {#each tracks('subtitle') as track}
              <option value={track.id}
                >{track.label || track.lang || track.id}</option
              >
            {/each}
          </select>
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
    position: absolute;
    inset: 0 0 auto;
    z-index: 1;
    min-height: 72%;
    overflow: hidden;
    pointer-events: none;
  }
  .backdrop {
    position: absolute;
    inset: 0;
    background-position: center 24%;
    background-size: cover;
    opacity: 0.82;
    animation: backdrop-in 420ms ease-out both;
  }
  .content {
    position: relative;
    width: min(44vw, 760px);
    display: grid;
    gap: clamp(12px, 1.5vh, 24px);
    margin: clamp(120px, 17vh, 210px) 0 0 clamp(28px, 5vw, 280px);
    pointer-events: auto;
    animation: content-in 320ms ease-out both;
  }
  h1,
  p {
    margin: 0;
  }
  h1 {
    max-width: 14ch;
    font-size: clamp(38px, 4.6vw, 112px);
    font-weight: 680;
    letter-spacing: -0.055em;
    line-height: 0.94;
    text-wrap: balance;
  }
  .metadata {
    display: flex;
    flex-wrap: wrap;
    gap: 7px 18px;
    color: rgba(255, 255, 255, 0.68);
    font-size: clamp(13px, 0.92vw, 22px);
  }
  .metadata span + span::before {
    content: '·';
    margin-right: 18px;
    color: rgba(255, 255, 255, 0.3);
  }
  .synopsis {
    width: min(100%, 690px);
    color: rgba(255, 255, 255, 0.78);
    font-size: clamp(14px, 1vw, 23px);
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
    max-height: min(44vh, 490px);
    overflow-y: auto;
    padding-right: 18px;
  }
  .expanded-copy button {
    margin-top: 18px;
    padding: 0;
    border: 0;
    color: #fff;
    background: transparent;
    font-weight: 650;
    cursor: pointer;
  }
  .selectors {
    min-height: 42px;
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }
  select {
    max-width: 260px;
    min-height: 40px;
    padding: 0 32px 0 13px;
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 8px;
    color: #fff;
    background: rgba(10, 10, 10, 0.72);
    backdrop-filter: blur(12px);
  }
  .preparing,
  .playback-error {
    color: rgba(255, 255, 255, 0.48);
    font-size: 13px;
  }
  .playback-error {
    color: #ffaaaa;
  }
  @keyframes backdrop-in {
    from {
      opacity: 0;
    }
  }
  @keyframes content-in {
    from {
      opacity: 0;
      transform: translateY(9px);
    }
  }
  @media (max-width: 900px) {
    .presentation {
      min-height: 70%;
    }
    .backdrop {
      background-position: 62% top;
    }
    .content {
      width: min(88vw, 620px);
      margin: 100px 20px 0;
    }
    h1 {
      font-size: clamp(34px, 9vw, 64px);
    }
    .synopsis {
      max-width: min(82vw, 570px);
    }
  }
  @media (max-height: 720px) and (min-width: 901px) {
    .content {
      margin-top: 100px;
      gap: 10px;
    }
    h1 {
      font-size: clamp(34px, 4vw, 68px);
    }
    .collapsed-copy {
      max-height: 5.8em;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .backdrop,
    .content {
      animation: none;
    }
  }
</style>

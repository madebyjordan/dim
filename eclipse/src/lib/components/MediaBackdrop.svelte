<script lang="ts">
  import type { Media } from '$lib/api/generated';
  import { imageUrl } from '$lib/catalog/catalog';

  let { media }: { media: Media } = $props();
  const backdrop = $derived(imageUrl(media.backdrop_path));
</script>

{#if backdrop}
  <div
    class="backdrop"
    style:background-image={`linear-gradient(180deg, transparent 0%, rgba(0, 0, 0, 0.35) 20%, #000000 95%), url("${backdrop}")`}
    aria-hidden="true"
  ></div>
{/if}

<style>
  .backdrop {
    position: absolute;
    inset: 0;
    z-index: -1;
    background-position: center 50%;
    background-size: cover;
    animation: backdrop-in 300ms ease-out both;
    pointer-events: none;
  }
  @keyframes backdrop-in {
    from {
      opacity: 0;
    }
  }
  @media (max-width: 700px) {
    .backdrop {
      background-position: 62% top;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .backdrop {
      animation: none;
    }
  }
</style>

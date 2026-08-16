<script lang="ts">
  import { onMount } from 'svelte';
  import EclipseMark from './EclipseMark.svelte';

  let { src, alt }: { src: string | null; alt: string } = $props();
  let host: HTMLDivElement;
  let visible = $state(false);
  let failed = $state(false);

  onMount(() => {
    if (!('IntersectionObserver' in window)) {
      visible = true;
      return;
    }
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) return;
        visible = true;
        observer.disconnect();
      },
      { rootMargin: '320px' }
    );
    observer.observe(host);
    return () => observer.disconnect();
  });
</script>

<div class="poster" bind:this={host}>
  {#if visible && src && !failed}
    <img {src} {alt} loading="lazy" onerror={() => (failed = true)} />
  {:else if visible}
    <span class="fallback"><EclipseMark /></span>
  {/if}
</div>

<style>
  .poster,
  img {
    width: 100%;
    height: 100%;
  }
  .poster {
    overflow: hidden;
    background: #141414;
  }
  img {
    display: block;
    object-fit: cover;
    animation: reveal 280ms ease-out both;
  }
  .fallback {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    opacity: 0.42;
  }
  @keyframes reveal {
    from {
      opacity: 0;
    }
  }
</style>

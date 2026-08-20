<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import PlaybackProof from '$lib/playback/PlaybackProof.svelte';
</script>

<svelte:head><title>Playback · Eclipse</title></svelte:head>
{#key page.params.fileId}
  <PlaybackProof
    fileId={page.params.fileId ?? ''}
    initialVideo={page.url.searchParams.get('video') ?? ''}
    initialAudio={page.url.searchParams.get('audio') ?? ''}
    initialSubtitle={page.url.searchParams.get('subtitle') ?? ''}
    onexit={() => void goto('/')}
  />
{/key}

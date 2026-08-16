<script lang="ts">
  import { page } from '$app/state';
  import PlaybackProof from '$lib/playback/PlaybackProof.svelte';
</script>

<svelte:head><title>Playback · Eclipse</title></svelte:head>
<PlaybackProof
  fileId={page.params.fileId ?? ''}
  initialVideo={page.url.searchParams.get('video') ?? ''}
  initialAudio={page.url.searchParams.get('audio') ?? ''}
  initialSubtitle={page.url.searchParams.get('subtitle') ?? ''}
/>

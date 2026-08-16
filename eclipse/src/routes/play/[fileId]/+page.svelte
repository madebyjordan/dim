<script lang="ts">
  import { page } from '$app/state';
  import PlaybackProof from '$lib/playback/PlaybackProof.svelte';
</script>

<svelte:head><title>Playback proof · Eclipse</title></svelte:head>
<PlaybackProof fileId={page.params.fileId ?? ''} />

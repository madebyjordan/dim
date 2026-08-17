<script lang="ts">
  import '../app.css';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { onMount } from 'svelte';
  import { session } from '$lib/auth/session.svelte';
  import { realtime } from '$lib/realtime/socket.svelte';
  import Button from '$lib/primitives/Button.svelte';

  let { children } = $props();
  let realtimeStarted = false;

  onMount(() => {
    let active = true;
    void session.bootstrap().then(() => {
      if (!active) return;
      if (session.status === 'authenticated') {
        if (page.url.pathname === '/login') void goto('/');
      } else if (
        session.status === 'anonymous' &&
        page.url.pathname !== '/login'
      ) {
        void goto('/login');
      }
    });
    return () => {
      active = false;
      realtime.stop();
    };
  });

  $effect(() => {
    if (session.status === 'authenticated' && !realtimeStarted) {
      realtime.start(
        () => session.token,
        () => session.expire()
      );
      realtimeStarted = true;
    } else if (session.status !== 'authenticated' && realtimeStarted) {
      realtime.stop();
      realtimeStarted = false;
    }
    if (session.status === 'anonymous' && page.url.pathname !== '/login')
      void goto('/login');
  });
</script>

{#if session.status === 'loading'}
  <main class="bootstrap">
    <div class="mark">E</div>
    <p>Restoring session…</p>
  </main>
{:else if session.status === 'error'}
  <main class="bootstrap error">
    <div class="mark">!</div>
    <h1>Eclipse could not start</h1>
    <p>{session.error}</p>
    <Button tone="surface" onclick={() => session.bootstrap()}>Retry</Button>
  </main>
{:else}
  {@render children()}
{/if}

<style>
  .bootstrap {
    min-height: 100vh;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 0.8rem;
    color: var(--color-fg-muted);
  }
  .mark {
    width: 48px;
    height: 48px;
    display: grid;
    place-items: center;
    border-radius: 14px;
    color: #08100d;
    background: var(--color-accent);
    font-weight: 900;
  }
  .error .mark {
    background: var(--color-danger);
  }
  h1,
  p {
    margin: 0;
  }
</style>

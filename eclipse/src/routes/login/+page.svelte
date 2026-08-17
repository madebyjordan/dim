<script lang="ts">
  import { goto } from '$app/navigation';
  import { session } from '$lib/auth/session.svelte';

  let username = $state('');
  let password = $state('');
  let submitting = $state(false);
  let error = $state<string | null>(null);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    submitting = true;
    error = null;
    try {
      await session.login({ username, password });
      await goto('/');
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Sign in failed';
    } finally {
      submitting = false;
    }
  }
</script>

<svelte:head><title>Sign in · Eclipse</title></svelte:head>
<main class="login">
  <form onsubmit={submit}>
    <p>Eclipse</p>
    <h1>Welcome back</h1>
    <label
      >Username<input
        bind:value={username}
        autocomplete="username"
        required
      /></label
    >
    <label
      >Password<input
        bind:value={password}
        type="password"
        autocomplete="current-password"
        required
      /></label
    >
    {#if error}<div class="error" role="alert">{error}</div>{/if}
    <button disabled={submitting}
      >{submitting ? 'Signing in…' : 'Sign in'}</button
    >
  </form>
</main>

<style>
  .login {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: var(--space-5);
  }
  form {
    width: min(100%, 360px);
    display: grid;
    gap: var(--space-4);
    padding: var(--space-6);
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow: var(--shadow-float);
  }
  p {
    margin: 0;
    color: var(--color-accent);
    font-weight: 800;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }
  h1 {
    margin: calc(-1 * var(--space-2)) 0 var(--space-2);
    font-size: 32px;
  }
  label {
    display: grid;
    gap: var(--space-2);
    color: var(--color-fg-muted);
    font-size: var(--text-sm);
  }
  input,
  button {
    min-height: var(--control-height-large);
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-md);
    padding: 0 0.8rem;
    color: var(--color-fg);
    background: var(--color-canvas);
  }
  button {
    border-color: var(--color-accent);
    color: #08100d;
    background: var(--color-accent);
    font-weight: 750;
    cursor: pointer;
  }
  .error {
    color: var(--color-danger);
    font-size: var(--text-sm);
  }
</style>

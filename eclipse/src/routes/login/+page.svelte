<script lang="ts">
  import { goto } from '$app/navigation';
  import { session } from '$lib/auth/session.svelte';
  import { onMount } from 'svelte';

  type OwnerState = 'loading' | 'exists' | 'missing' | 'error';

  let ownerState = $state<OwnerState>('loading');
  let username = $state('');
  let password = $state('');
  let confirmPassword = $state('');
  let submitting = $state(false);
  let error = $state<string | null>(null);

  onMount(() => {
    void checkOwner();
  });

  async function checkOwner() {
    ownerState = 'loading';
    error = null;
    try {
      ownerState = (await session.adminExists()) ? 'exists' : 'missing';
    } catch (cause) {
      ownerState = 'error';
      error =
        cause instanceof Error
          ? cause.message
          : 'Eclipse could not check account setup';
    }
  }

  async function submitLogin(event: SubmitEvent) {
    event.preventDefault();
    if (submitting) return;
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

  async function submitRegistration(event: SubmitEvent) {
    event.preventDefault();
    if (submitting) return;
    error = null;
    if (password !== confirmPassword) {
      error = 'Passwords do not match';
      return;
    }
    submitting = true;
    try {
      await session.register({ username, password });
      await goto('/');
    } catch (cause) {
      error =
        cause instanceof Error ? cause.message : 'Account creation failed';
    } finally {
      submitting = false;
    }
  }
</script>

<svelte:head><title>Sign in · Eclipse</title></svelte:head>
<main class="login">
  {#if ownerState === 'loading'}
    <section class="card status" aria-busy="true">
      <p>Eclipse</p>
      <h1>Welcome to Eclipse</h1>
      <div class="setup-status" role="status">Checking account setup…</div>
    </section>
  {:else if ownerState === 'error'}
    <section class="card status">
      <p>Eclipse</p>
      <h1>Welcome to Eclipse</h1>
      {#if error}<div class="error" role="alert">{error}</div>{/if}
      <button type="button" onclick={() => void checkOwner()}>Retry</button>
    </section>
  {:else if ownerState === 'missing'}
    <form class="card" onsubmit={submitRegistration}>
      <p>Eclipse</p>
      <h1>Welcome to Eclipse</h1>
      <h2>Create your account</h2>
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
          autocomplete="new-password"
          minlength="8"
          required
        /></label
      >
      <label
        >Confirm password<input
          bind:value={confirmPassword}
          type="password"
          autocomplete="new-password"
          minlength="8"
          required
        /></label
      >
      {#if error}<div class="error" role="alert">{error}</div>{/if}
      <button disabled={submitting}
        >{submitting ? 'Creating account…' : 'Create account'}</button
      >
    </form>
  {:else}
    <form class="card" onsubmit={submitLogin}>
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
  {/if}
</main>

<style>
  .login {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: var(--space-5);
  }
  .card {
    width: min(100%, 360px);
    display: grid;
    gap: var(--space-4);
    padding: var(--space-6);
    border: 1px solid var(--color-stroke);
    border-radius: var(--radius-lg);
    background: var(--color-surface);
    box-shadow: var(--shadow-float);
  }
  .status {
    min-height: 200px;
    align-content: center;
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
  h2 {
    margin: calc(-1 * var(--space-2)) 0 0;
    color: var(--color-fg-muted);
    font-size: var(--text-lg);
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
    color: var(--color-on-accent);
    background: var(--color-accent);
    font-weight: 750;
    cursor: pointer;
  }
  .error {
    color: var(--color-danger);
    font-size: var(--text-sm);
  }
  .setup-status {
    color: var(--color-fg-muted);
  }
</style>

<script lang="ts">
  export type SelectOption = { value: string; label: string };

  type Props = {
    label: string;
    value: string;
    options: SelectOption[];
    disabled?: boolean;
    onvaluechange: (value: string) => void;
  };

  let {
    label,
    value,
    options,
    disabled = false,
    onvaluechange
  }: Props = $props();
  const componentId = $props.id();
  const selectId = `select-${componentId}`;
  const hasSelectedOption = $derived(
    options.some((option) => option.value === value)
  );
  const unavailable = $derived(disabled || options.length === 0);
</script>

<div class="select">
  <label for={selectId}>{label}</label>
  <select
    id={selectId}
    disabled={unavailable}
    {value}
    onchange={(event) => onvaluechange(event.currentTarget.value)}
  >
    {#if !hasSelectedOption}
      <option {value} disabled hidden>{label}</option>
    {/if}
    {#each options as option}
      <option value={option.value}>{option.label}</option>
    {/each}
  </select>
  <svg viewBox="0 0 20 20" aria-hidden="true">
    <path d="m4 7 6 6 6-6" />
  </svg>
</div>

<style>
  .select {
    position: relative;
    display: inline-grid;
    min-width: 0;
    max-width: 100%;
  }

  label {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  select {
    width: 100%;
    min-height: var(--control-height);
    padding: 0 calc(var(--space-4) + 20px) 0 var(--space-4);
    border: 0;
    border-radius: var(--radius-round);
    color: var(--color-fg);
    font: inherit;
    background: var(--color-control);
    -webkit-backdrop-filter: blur(var(--blur-control));
    backdrop-filter: blur(var(--blur-control));
    appearance: none;
    white-space: nowrap;
    cursor: pointer;
  }

  select:disabled {
    opacity: var(--opacity-disabled);
    cursor: default;
  }

  svg {
    position: absolute;
    top: 50%;
    right: var(--space-3);
    width: 20px;
    height: 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    pointer-events: none;
    transform: translateY(-50%);
  }
</style>

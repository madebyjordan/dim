<script lang="ts">
  import Select from './Select.svelte';

  const options = [{ value: 'one', label: 'One' }];
</script>

<Select label="First track" value="one" {options} onvaluechange={() => {}} />
<Select label="Second track" value="one" {options} onvaluechange={() => {}} />

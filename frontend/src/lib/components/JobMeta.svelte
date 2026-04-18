<script lang="ts">
  let { repo, branch }: { repo: string; branch: string | null } = $props()
</script>

<div class="run-card-meta truncate">
  {#if branch !== null && branch !== ''}
    {repo} <span aria-hidden="true">·</span> {branch}
  {:else}
    {repo}
  {/if}
</div>

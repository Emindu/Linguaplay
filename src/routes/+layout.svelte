<script lang="ts">
  import "../lib/theme.css";
  import { loadThemeSettings, applyTheme } from "$lib/themeSettings";

  // Runs synchronously during layout construction, before anything paints —
  // avoids a flash of the default theme before the stored one is restored.
  applyTheme(loadThemeSettings());

  let { children } = $props();
</script>

{@render children()}

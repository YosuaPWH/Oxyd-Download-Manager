<script lang="ts">
  import { onMount } from "svelte";
  import SettingsPage from "./components/SettingsPage.svelte";
  import { api } from "./lib/api";
  import { t } from "./lib/i18n.svelte";
  import { app, initSettings, run } from "./lib/store.svelte";

  onMount(() => {
    void run(async () => {
      const [info] = await Promise.all([api.appInfo(), initSettings()]);
      app.version = info.version;
    });
  });
</script>

{#if app.settings}
  <SettingsPage />
{:else}
  <p role={app.error ? "alert" : "status"}>{app.error ?? t("loading")}</p>
{/if}

<style>
  p {
    padding: 0 20px;
  }
</style>

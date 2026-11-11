<script lang="ts">
  import { api, errorMessage } from "../lib/api";
  import * as fmt from "../lib/format";
  import { t, type Key } from "../lib/i18n.svelte";
  import { MAIN_QUEUE, type Queue } from "../lib/types";
  import Modal from "./Modal.svelte";

  let { queue, onclose }: { queue: Queue; onclose: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let q = $state<Queue>(structuredClone($state.snapshot(queue)));
  // svelte-ignore state_referenced_locally
  let limit = $state(fmt.rateInput(queue.speedLimit));
  let error = $state<string | null>(null);
  let busy = $state(false);
  let confirmDelete = $state(false);

  const isMain = $derived(q.id === MAIN_QUEUE);
  const isNew = $derived(!q.id);
  const DAYS = [0, 1, 2, 3, 4, 5, 6];

  function toggleDay(d: number) {
    const days = q.schedule.days;
    q.schedule.days = days.includes(d) ? days.filter((x) => x !== d) : [...days, d].sort();
  }

  async function save() {
    busy = true;
    error = null;
    try {
      await api.saveQueue({
        ...$state.snapshot(q),
        name: q.name.trim(),
        speedLimit: isMain ? null : fmt.parseRate(limit),
        schedule: { ...$state.snapshot(q.schedule), start: q.schedule.start || null, stop: q.schedule.stop || null },
      });
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    busy = true;
    try {
      await api.deleteQueue(q.id);
      onclose();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={isNew ? t("queue.new") : isMain ? t("queue.main") : t("queue.edit")} {onclose} width={480}>
  <form class="stack" onsubmit={(e) => (e.preventDefault(), void save())}>
    {#if !isMain}
      <div>
        <label for="q-name">{t("queue.name")}</label>
        <input id="q-name" bind:value={q.name} maxlength="40" placeholder={t("queue.namePlaceholder")} />
      </div>
    {/if}
    <div class="two">
      <div>
        <label for="q-max">{t("queue.maxActive")}</label>
        <input id="q-max" class="num" type="number" min="1" max="20" bind:value={q.maxActive} />
      </div>
      {#if !isMain}
        <div>
          <label for="q-limit">{t("queue.limit")}</label>
          <input id="q-limit" class="num" bind:value={limit} placeholder={t("limit.off")} title={t("detail.limitHint")} />
        </div>
      {/if}
    </div>

    <fieldset>
      <legend>
        <label class="check">
          <input type="checkbox" class="switch" role="switch" bind:checked={q.schedule.enabled} />{t("set.scheduleEnabled")}
        </label>
      </legend>
      <div class="two">
        <div>
          <label for="q-start">{t("set.scheduleStart")}</label>
          <input id="q-start" class="num" type="time" bind:value={q.schedule.start} disabled={!q.schedule.enabled} />
        </div>
        <div>
          <label for="q-stop">{t("set.scheduleStop")}</label>
          <input id="q-stop" class="num" type="time" bind:value={q.schedule.stop} disabled={!q.schedule.enabled} />
        </div>
      </div>
      <div class="days" role="group" aria-label={t("set.scheduleDays")}>
        {#each DAYS as d (d)}
          <button type="button" aria-pressed={q.schedule.days.includes(d)} disabled={!q.schedule.enabled} onclick={() => toggleDay(d)}>
            {t(`day.${d}` as Key)}
          </button>
        {/each}
      </div>
      <p class="faint hint">{t("set.scheduleDaysHint")}</p>
    </fieldset>

    {#if confirmDelete}
      <div class="note" role="alert">
        <span>{t("queue.deleteHint")}</span>
        <button type="button" class="danger" onclick={remove} disabled={busy}>{t("queue.delete")}</button>
      </div>
    {/if}
    {#if error}<div class="errbox" role="alert">{error}</div>{/if}
    <button type="submit" hidden aria-hidden="true" tabindex="-1"></button>
  </form>

  {#snippet footer()}
    {#if !isMain && !isNew && !confirmDelete}
      <button class="danger left" onclick={() => (confirmDelete = true)}>{t("queue.delete")}</button>
    {/if}
    <button onclick={onclose}>{t("new.cancel")}</button>
    <button class="primary" onclick={save} disabled={busy || (!isMain && !q.name.trim())}>{t("queue.save")}</button>
  {/snippet}
</Modal>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  label[for] {
    display: block;
    margin-bottom: 6px;
    font-size: 12px;
    line-height: 16px;
    color: var(--muted);
  }
  input:not([type="checkbox"]) {
    width: 100%;
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }
  fieldset {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin: 0;
    padding: 12px 14px;
    border: 0;
    border-radius: var(--radius);
    background: var(--raised);
  }
  legend {
    float: left;
    width: 100%;
    padding: 0;
  }
  .days {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .days button {
    min-width: 40px;
    padding: 0 6px;
    font-size: 12px;
    color: var(--muted);
    border-color: var(--border);
  }
  .days button[aria-pressed="true"] {
    color: var(--accent);
    font-weight: 600;
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }
  .hint {
    margin: 0;
    font-size: 12px;
  }
  .note {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 8px 8px 12px;
    border-radius: var(--radius);
    background: var(--neg-bg);
  }
  .errbox {
    padding: 8px 12px;
    border-radius: var(--radius);
    background: var(--neg-bg);
    color: var(--negative);
  }
  :global(footer) .left {
    margin-right: auto;
  }
</style>

<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/tauri";
  import { onDestroy, onMount } from "svelte";
  import { updateConfig, type Config } from "$lib/config";
  import { fade } from "svelte/transition";
  import RevealCount from "./reveal-count.svelte";
  import type { ChampSelect } from "$lib/champ_select";
  import { Switch } from "./ui/switch";
  import { Label } from "./ui/label";
  import { Button } from "./ui/button";
  import * as Select from "$lib/components/ui/select";
  import type { PostGameSummary } from "$lib/post-game";

  export let config: Config | null = null;
  export let state = "Unknown";
  export let champSelect: ChampSelect | null = null;
  export let connected = false;

  let lastSecondDodgeEnabled = false;
  let processingLastGame = false;
  let postGameSummary: PostGameSummary | null = null;
  let postGameError: string | null = null;
  let unlistenProcessed: (() => void) | null = null;
  let unlistenFailed: (() => void) | null = null;
  $: if (state !== "ChampSelect" && lastSecondDodgeEnabled) {
    // lobby is prob dodged or started, can reset state now
    lastSecondDodgeEnabled = false;
  }

  const multiProviders = [
    {
      label: "OP.GG",
      value: "opgg",
    },
    {
      label: "DeepLoL",
      value: "deeplol",
    },
    {
      label: "U.GG",
      value: "ugg",
    },
    {
      label: "Tracker.gg",
      value: "tracker",
    },
  ];

  async function handleProcessLastGame() {
    processingLastGame = true;
    postGameError = null;
    try {
      postGameSummary = await invoke<PostGameSummary>("process_last_game");
    } catch (error) {
      postGameSummary = null;
      if (typeof error === "string") {
        postGameError = error;
      } else if (error && typeof error === "object" && "message" in error) {
        postGameError = String((error as { message?: unknown }).message);
      } else {
        postGameError = "Failed to process last game.";
      }
    } finally {
      processingLastGame = false;
    }
  }

  function setPostGameSummary(summary: PostGameSummary) {
    postGameSummary = summary;
    postGameError = null;
  }

  onMount(async () => {
    unlistenProcessed = await listen<PostGameSummary>(
      "post_game_processed",
      (event) => {
        setPostGameSummary(event.payload);
      }
    );

    unlistenFailed = await listen<string>("post_game_failed", (event) => {
      postGameSummary = null;
      postGameError = event.payload;
    });
  });

  onDestroy(() => {
    unlistenProcessed?.();
    unlistenFailed?.();
  });
</script>

<div class="flex flex-col gap-2">
  <div class="flex gap-5 items-center">
    <div>
      <Label for="favoriteFruit">Multi Website</Label>
      <Select.Root
        onSelectedChange={(v) => {
          if (!config) return;
          if (v) {
            config.multiProvider = v.value;
          }
          updateConfig(config);
        }}
        selected={multiProviders.find((p) => p.value === config?.multiProvider)}
      >
        <Select.Trigger class="w-[180px]">
          <Select.Value />
        </Select.Trigger>
        <Select.Content>
          <Select.Group>
            {#each multiProviders as multi}
              <Select.Item value={multi.value} label={multi.label}
                >{multi.label}</Select.Item
              >
            {/each}
          </Select.Group>
        </Select.Content>
      </Select.Root>
    </div>
    <div class="flex flex-col gap-3">
      <div class="flex items-center space-x-2">
        <Switch
          checked={config?.autoOpen}
          id="auto-open"
          onCheckedChange={(v) => {
            if (!config) return;
            config.autoOpen = v;
            updateConfig(config);
          }}
        />
        <Label for="auto-open">Auto Open Multi</Label>
      </div>
      <div class="flex items-center space-x-2">
        <Switch
          checked={config?.autoAccept}
          id="auto-accept"
          onCheckedChange={(v) => {
            if (!config) return;
            config.autoAccept = v;
            updateConfig(config);
          }}
        />
        <Label for="auto-accept">Auto Accept</Label>
      </div>
      <div class="flex items-center space-x-2">
        <Switch
          checked={config?.autoReportNonFriends}
          id="auto-report-non-friends"
          onCheckedChange={(v) => {
            if (!config) return;
            config.autoReportNonFriends = v;
            updateConfig(config);
          }}
        />
        <Label for="auto-report-non-friends">Auto Report Non-Friends</Label>
      </div>
    </div>
  </div>
  <div class="grid grid-cols-2 text-sm">
    <div class="flex flex-col">
      <div class="text-muted-foreground text-xs">State</div>
      <div>{state}</div>
    </div>
    <div class="flex flex-col">
      <div class="text-muted-foreground text-xs">Revealed Champ Selects</div>
      <div>
        <RevealCount />
      </div>
    </div>
  </div>
  <div class="flex flex-col gap-2">
    <Button
      class="w-full md:w-[220px]"
      size="sm"
      disabled={!connected || processingLastGame}
      on:click={handleProcessLastGame}
    >
      {#if processingLastGame}
        Processing Last Game...
      {:else}
        Process Last Game
      {/if}
    </Button>
    {#if postGameSummary}
      <div class="rounded-md border p-2 text-xs bg-primary-foreground">
        <div class="font-medium">Processed Game {postGameSummary.gameId}</div>
        {#if postGameSummary.players.length > 0}
          <div class="mt-2 flex flex-col gap-2">
            {#each postGameSummary.players as player}
              <div class="flex justify-between gap-4 rounded border bg-secondary/30 p-2">
                <div class="flex flex-col gap-1">
                  <div class="font-medium">
                    {player.gameName ?? player.summonerName ?? player.puuid}
                    {#if player.tagLine}
                      #{player.tagLine}
                    {/if}
                  </div>
                  {#if player.summonerName && player.summonerName !== player.gameName}
                    <div class="text-muted-foreground">
                      Summoner: {player.summonerName}
                    </div>
                  {/if}
                </div>
                <div class="flex flex-col items-end gap-1 text-[11px] text-muted-foreground">
                  <span>
                    Friend Request:
                    <span class={player.friendRequestSent ? "text-foreground" : "text-destructive"}>
                      {player.friendRequestSent ? "Sent" : "Failed"}
                    </span>
                  </span>
                  <span>
                    Report:
                    <span class={player.reportSent ? "text-foreground" : "text-destructive"}>
                      {player.reportSent ? "Sent" : "Failed"}
                    </span>
                  </span>
                </div>
              </div>
            {/each}
          </div>
        {:else}
          <div class="mt-1 text-muted-foreground">
            No non-friends found to process.
          </div>
        {/if}
      </div>
    {/if}
    {#if postGameError}
      <div class="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
        {postGameError}
      </div>
    {/if}
  </div>
  {#if state === "ChampSelect"}
    <div in:fade class="flex flex-col gap-5 w-full">
      {#if champSelect}
        <div class="grid grid-cols-2 items-start gap-y-1 gap-x-2 text-sm">
          {#each champSelect.participants as participant}
            <div
              class="flex flex-col items-center border bg-primary-foreground rounded-md text-xs h-9"
            >
              <div class="line-clamp-1">
                {participant.game_name}#{participant.game_tag}
              </div>
              {#if participant.name}
                <div class="flex text-muted-foreground">
                  ({participant.name})
                </div>
              {/if}
            </div>
          {/each}
        </div>
        <Button
          class="h-9 absolute right-4 w-[180px] bottom-[52px]"
          size="sm"
          on:click={() => invoke("open_opgg_link")}
        >
          Open Multi Link
        </Button>
      {:else}
        <div class="grid grid-cols-2 items-start gap-y-1 gap-x-2 text-sm">
          {#each new Array(5) as _}
            <div
              class="bg-primary-foreground border animate-pulse h-9 w-full rounded-md"
            />
          {/each}
        </div>
        <Button
          class="h-9 hover:cursor-not-allowed absolute right-4 w-[180px] bottom-[52px]"
          size="sm"
          on:click={() => invoke("open_opgg_link")}
        >
          Open Multi Link
        </Button>
      {/if}
    </div>
  {:else if state === "InProgress"}
    <div in:fade class="flex gap-2 items-center animate-pulse">In Game</div>
  {:else if !connected}
    <div in:fade class="flex gap-2 items-center animate-pulse">
      Trying to find League Client...
    </div>
    <div
      class="text-xs p-2 rounded bg-accent border flex gap-2 text-muted-foreground"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="20"
        height="20"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        class="lucide lucide-info"
        ><circle cx="12" cy="12" r="10" /><path d="M12 16v-4" /><path
          d="M12 8h.01"
        /></svg
      >
      Issues Connecting? <br /> Try restarting the League Client and running Reveal
      as Administrator.
    </div>
  {:else}
    <div in:fade class="flex gap-2 items-center animate-pulse">
      Waiting for Champ Select...
    </div>
  {/if}
</div>

<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/tauri";
  import { type Config } from "$lib/config";
  import "@fontsource-variable/inter";
  import type { ChampSelect } from "$lib/champ_select";
  import Tool from "$lib/components/tool.svelte";
  import Navbar from "$lib/components/navbar.svelte";
  import Footer from "$lib/components/footer.svelte";
  // Updater removed; no longer using @tauri-apps/api/updater or process

  let state = "Unknown";
  let connected = false;
  let champSelect: ChampSelect | null = null;
  let config: Config | null = null;
  // Updater UI removed; show Tool immediately

  onMount(async () => {
    await listen<string>("client_state_update", (event) => {
      const newState = event.payload;
      if (newState === "ChampSelect") {
        champSelect = null;
      }

      state = newState;
    });

    await listen<boolean>("lcu_state_update", (event) => {
      connected = event.payload;
    });

    await listen<ChampSelect>("champ_select_started", (event) => {
      champSelect = event.payload;
    });

    await listen("preendofgame", () => {
      console.log("auto_report: pre-end detected");
    });

    await listen("endofgame", () => {
      console.log("auto_report: end-of-game detected");
    });

    config = await invoke<Config>("app_ready");

  });
</script>

<main class="h-[325px] bg-background border rounded-md">
  <Navbar />
  <div class="h-[240px] px-4 pt-1">
    <Tool {config} {state} {champSelect} {connected} />
  </div>
  <Footer {connected} />
</main>

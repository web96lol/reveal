<script lang="ts">
    import { getVersion } from "@tauri-apps/api/app";
    import { appWindow } from "@tauri-apps/api/window";

    // Static arrow direction (bottom arrow, but you can set it to "top" if you want the arrow on top)
    let arrowDirection: "bottom" = "bottom";

    // Fade state
    let closing = false;

    async function hideToTray() {
        closing = true;
        setTimeout(async () => {
            await appWindow.hide();
            closing = false;
        }, 160);
    }
</script>

<style>
    /* Fade out animation */
    .fade-out {
        opacity: 0;
        transition: opacity 0.15s ease-out;
    }

    /* Base arrow */
    .arrow {
        position: absolute;
        width: 0;
        height: 0;
    }

    /* Static Arrow positions */
    .arrow-bottom {
        bottom: 100%;
        left: 50%;
        transform: translateX(-50%);
        border-left: 8px solid transparent;
        border-right: 8px solid transparent;
        border-bottom: 8px solid rgba(0, 0, 0, 0.85);
    }
</style>

<!-- NAVBAR -->
<div
    data-tauri-drag-region
    class="relative flex border-b rounded-t-lg w-full select-none px-4 py-2 bg-[#141414]/90"
    class:fade-out={closing}
>
    <div class="flex items-center gap-2">
        <img alt="" src="/icon.png" class="w-5 h-5" />
        <div class="text-blue-500">reveal</div>

        {#await getVersion()}
            <div></div>
        {:then version}
            <div class="text-gray-400">v{version}</div>
        {:catch error}
            <div>{error.message}</div>
        {/await}
    </div>

    <div class="ml-auto flex gap-2">
        <!-- MINIMIZE -->
        <button on:click={() => appWindow.minimize()}>
            –
        </button>

        <!-- CLOSE → Hide to tray -->
        <button
            class="text-xs hover:text-red-400"
            on:click={hideToTray}
        >
            X
        </button>
    </div>

    <!-- 🎯 STATIC ARROW (bottom arrow for simplicity) -->
    {#if arrowDirection === "bottom"}
        <div class="arrow arrow-bottom"></div>
    {/if}
</div>

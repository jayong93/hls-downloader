<script lang="ts">
    import Select, { Option } from "@smui/select";
    import Button from "@smui/button";
    import CircularProgress from "@smui/circular-progress";
    import type { DownloadableVideo } from "./interface";
    import { listen, Event } from "@tauri-apps/api/event";

    export let list: DownloadableVideo[], disabled: boolean;

    $: progresses = Array(list.length).fill(0);

    listen("Progress", (event: Event<[number, number]>) => {
        progresses[event.payload[0]] += event.payload[1];
    });

    function remove(i: number) {
        list.splice(i, 1);
        list = list;
    }
</script>

<div class="list">
    {#each list as video, i}
        <div class="container">
            <div class="texts">
                <div class="name">
                    {video.file_name}
                </div>
                <div class="url">
                    {video.hls_url}
                </div>
            </div>
            <div class="controls">
                <div class="bandwidth">
                    {#if video.bandwidths && video.bandwidths.length > 0}
                        <Select
                            {disabled}
                            bind:value={video.selected_bandwidth}
                        >
                            {#each video.bandwidths as data}
                                <Option value={data}>{data.bandwidth}</Option>
                            {/each}
                            <svelte:fragment slot="helperText"
                                >Bandwidth</svelte:fragment
                            >
                        </Select>
                    {/if}
                </div>
                <div class="remove">
                    {#if disabled}
                        <CircularProgress
                            style="height: 48px; width: 48px"
                            progress={progresses[i]}
                        />
                    {:else}
                        <Button
                            on:click={() => remove(i)}
                            variant="outlined">Remove</Button
                        >
                    {/if}
                </div>
            </div>
        </div>
    {/each}
</div>

<style>
    div.list {
        width: 100%;
        margin: 5px;
    }

    div.container {
        display: flex;
        justify-content: space-between;
        align-items: center;
    }

    div.texts {
        display: inline-block;
        width: 60%;
        text-align: left;
        overflow: hidden;
        text-overflow: ellipsis;
        margin-right: 10px;
    }

    div.controls {
        display: inline-grid;
        grid-template-rows: 1fr;
        grid-template-columns: 1fr 1fr;
        width: 40%;
        align-items: center;
    }
    div.url {
        color: gray;
        font-size: small;
    }

    div.name {
        font-size: medium;
    }

    div.remove {
        align-items: center;
        grid-row: 1/2;
        grid-column: 2/3;
    }

    div.bandwidth {
        align-items: center;
        grid-column: 1/2;
    }
</style>

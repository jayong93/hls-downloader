<script>
    import { invoke } from "@tauri-apps/api/tauri";

    export let list;

    function select_file() {
        invoke("get_save_file_name").then((path) => {
            file_name = path;
        });
    }

    function remove(i) {
        list.splice(i, 1);
        list = list;
    }
</script>

<div class="list">
    <ul>
        {#each list as video, i}
            <li>
                <input type="text" value={video.hls_url} readonly />
                <input type="text" value={video.file_name} readonly />
                {#if video.range_start}
                    <input type="text" value={video.range_start} readonly />
                {/if}
                {#if video.range_end}
                    <input type="text" value={video.range_end} readonly />
                {/if}
                {#if video.bandwidths && video.bandwidths.length > 0}
                    <select bind:value={video.selected_bandwidth}>
                        {#each video.bandwidths as data}
                            <option value={data}>{data.bandwidth}</option>
                        {/each}
                    </select>
                {/if}
                <button on:click={() => remove(i)}>Remove</button>
            </li>
        {/each}
    </ul>
</div>

<style>
    div.list {
        width: 100%;
    }
</style>

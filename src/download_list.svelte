<script>
    import List, {
        Item,
        Text,
        PrimaryText,
        SecondaryText,
        Meta,
    } from "@smui/list";
    import Select, { Option } from "@smui/select";
    import Button from "@smui/button";

    export let list;

    function remove(i) {
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
                        <Select bind:value={video.selected_bandwidth}>
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
                    <Button on:click={() => remove(i)} variant="outlined"
                        >Remove</Button
                    >
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

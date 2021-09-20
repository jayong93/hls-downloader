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
                {#if video.bandwidths && video.bandwidths.length > 0}
                    <Select
                        bind:value={video.selected_bandwidth}
                    >
                        {#each video.bandwidths as data}
                            <Option value={data}>{data.bandwidth}</Option>
                        {/each}
                    </Select>
                {/if}
                <Button on:click={() => remove(i)} variant="outlined"
                    >Remove</Button
                >
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
    }

    div.controls {
        display: inline-block;
        width: 40%;
    }
    div.url {
        color: gray;
        font-size: small;
    }

    div.name {
        font-size: medium;
    }
</style>

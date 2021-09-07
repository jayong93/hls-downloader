<script>
    import { invoke } from "@tauri-apps/api/tauri";
    import { createEventDispatcher } from "svelte";
    import { readText } from "@tauri-apps/api/clipboard";
    import { register, unregister } from "@tauri-apps/api/globalShortcut";

    export let disabled = false;

    let hls_url;
    let file_name;
    let range_start;
    let range_end;

    let url_input;
    let range_start_input;
    let range_end_input;

    let url_input_focused = false;

    $: is_valid =
        hls_url &&
        url_input.checkValidity() &&
        file_name &&
        (!range_start || range_start_input.checkValidity()) &&
        (!range_end || range_end_input.checkValidity());

    function select_file() {
        invoke("get_save_file_name").then((path) => {
            file_name = path;
        });
    }

    const dispatch = createEventDispatcher();
    function add() {
        invoke("add_video", { videoUrl: hls_url })
            .then((bandwidths) => {
                let result = bandwidths.map((v, i) => {
                    return { idx: i, bandwidth: v };
                });
                dispatch("Add", {
                    hls_url,
                    bandwidths: result,
                    range_start,
                    range_end,
                    file_name,
                });
                hls_url = undefined;
                file_name = undefined;
                range_start = undefined;
                range_end = undefined;
            })
            .catch((err) => alert(err));
    }

    function paste_url(_shortcut) {
        if (url_input_focused) {
            readText().then((text) => (hls_url = text));
        }
    }
    register("CmdOrControl+V", paste_url).catch((e) => alert(e));
</script>

<div class="input url">
    <input
        type="url"
        bind:value={hls_url}
        name="url"
        placeholder="HLS URL"
        bind:this={url_input}
        on:focus={() => (url_input_focused = true)}
        on:blur={() => (url_input_focused = false)}
    />
</div>
<div class="input file">
    <input
        type="text"
        bind:value={file_name}
        name="file_name"
        placeholder="File Name"
        readonly
        on:click={select_file}
    />
</div>
<div class="input">
    <input
        type="text"
        bind:value={range_start}
        name="range_start"
        pattern="\d+(:\d+){'{'}0,2{'}'}"
        placeholder="Start Time"
        title="aa"
        size="9"
        bind:this={range_start_input}
    />
</div>
<div class="input">
    <input
        type="text"
        bind:value={range_end}
        name="range_end"
        pattern="\d+(:\d+){'{'}0,2{'}'}"
        placeholder="End Time"
        title="aa"
        size="9"
        bind:this={range_end_input}
    />
</div>
<div class="input">
    <button disabled={is_valid && !disabled ? false : true} on:click={add}
        >Add</button
    >
</div>

<style>
    div.input {
        display: inline-block;
    }

    div.url {
        width: 30%;
    }

    input:invalid {
        color: red;
    }

    input[type="url"] {
        width: 100%;
    }
</style>

<script lang="ts">
    import { invoke } from "@tauri-apps/api/tauri";
    import { createEventDispatcher } from "svelte";
    import { readText } from "@tauri-apps/api/clipboard";
    import { register, unregister } from "@tauri-apps/api/globalShortcut";
    import Button from "@smui/button";
    import Textfield from "@smui/textfield";
    import Card, { PrimaryAction } from "@smui/card";
    import isURL from 'validator/lib/isURL';

    export let disabled = false;

    let hls_url = "";
    let file_name;
    let range_start = "";
    let range_end = "";

    let url_invalid;
    let range_start_invalid;
    let range_end_invalid;

    let url_input_focused = false;

    $: is_valid =
        hls_url &&
        isURL(hls_url) &&
        file_name &&
        !range_end_invalid &&
        !range_start_invalid;

    function select_file() {
        invoke("get_save_file_name").then((path) => {
            file_name = path;
        });
    }

    const dispatch = createEventDispatcher();
    function add() {
        invoke("add_video", { videoUrl: hls_url })
            .then((bandwidths: number[]) => {
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
                hls_url = "";
                file_name = "";
                range_start = "";
                range_end = "";
            })
            .catch((err) => alert(err));
    }

    function paste_url(_shortcut) {
        if (url_input_focused) {
            readText().then((text) => (hls_url = text));
        }
    }

    unregister("CmdOrControl+V").finally(() => {
        register("CmdOrControl+V", paste_url).catch((e) => alert(e));
    });
</script>

<div class="input url">
    <Textfield
        style="width: 100%;"
        required
        type="url"
        variant="outlined"
        bind:value={hls_url}
        invalid={url_invalid}
        label="HLS Url"
        on:focus={() => (url_input_focused = true)}
        on:blur={() => (url_input_focused = false)}
    />
</div>
<div class="input file">
    <Card style="width: 100%;">
        <PrimaryAction padded on:click={select_file}>
            {file_name ? file_name : "Click To Save File"}
        </PrimaryAction>
    </Card>
</div>
<div class="input">
    <Textfield
        type="text"
        variant="outlined"
        bind:value={range_start}
        bind:invalid={range_start_invalid}
        updateInvalid
        name="range_start"
        input$pattern="\d+(:\d+){'{'}0,2{'}'}"
        label="Start Time"
    />
    <Textfield
        type="text"
        variant="outlined"
        bind:value={range_end}
        bind:invalid={range_end_invalid}
        updateInvalid
        name="range_end"
        input$pattern="\d+(:\d+){'{'}0,2{'}'}"
        label="End Time"
    />
</div>
<div class="input">
    <Button
        on:click={add}
        disabled={is_valid && !disabled ? false : true}
        variant="outlined">Add</Button
    >
</div>

<style>
    div.input {
        width: 100%;
        margin: 10px;
    }
</style>

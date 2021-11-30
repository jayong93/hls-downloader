<script lang="ts">
	import { invoke } from "@tauri-apps/api/tauri";
	import { listen } from "@tauri-apps/api/event";

	import Adder from "./adder.svelte";
	import VideoList from "./download_list.svelte";
	import type { DownloadableVideo } from "./interface";

	let video_list: DownloadableVideo[] = [];
	let log_msg = [];
	let is_downloading = false;

	function download() {
		if (is_downloading == false) {
			is_downloading = true;
			invoke("download", { videoList: video_list }).finally(() => {
				video_list = [];
				is_downloading = false;
			});
		}
	}

	listen("AddLog", (event) => {
		log_msg = [...log_msg, event.payload];
	});

	$: total_log =
		log_msg && log_msg.length > 0 ? log_msg.join("\n") : "Log Area";
</script>

<main>
	<Adder
		on:Add={(message) => {
			video_list = [...video_list, message.detail];
		}}
		disabled={is_downloading}
	/>
	<VideoList bind:list={video_list} disabled={is_downloading} />
	<div>
		<textarea
			class="log"
			disabled={!log_msg || log_msg.length == 0}
			value={total_log}
			readonly
		/>
	</div>
	<div>
		<button on:click={download} disabled={is_downloading}
			>Download All</button
		>
	</div>
</main>

<style>
	main {
		text-align: center;
		padding: 1em;
		width: 90%;
		height: 90%;
		margin: 0 auto;
	}

	@media (min-width: 640px) {
		main {
			max-width: none;
		}
	}

	.log {
		overflow-y: scroll;
		width: 100%;
		min-height: 10em;
		resize: none;
	}

	.log:disabled {
		color: gray;
	}
</style>
